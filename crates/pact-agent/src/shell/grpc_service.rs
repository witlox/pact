//! ShellService gRPC adapter — wraps the ShellServer for tonic.
//!
//! Maps proto ShellService RPCs to ShellServer methods.
//! Auth token extracted from gRPC metadata (authorization header).

use std::sync::Arc;

use tokio::sync::RwLock;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};
use tracing::info;
#[cfg(target_os = "linux")]
use tracing::warn;

#[cfg(target_os = "linux")]
use pact_common::proto::shell::shell_output;
use pact_common::proto::shell::{
    exec_output, shell_input, shell_service_server::ShellService, CommandEntry, ExecOutput,
    ExecRequest, ExtendWindowRequest, ExtendWindowResponse, ListCommandsRequest,
    ListCommandsResponse, ShellInput, ShellOutput,
};

use crate::commit::CommitWindowManager;
use crate::shell::auth::has_ops_role;

use super::ShellServer;

/// gRPC ShellService implementation — delegates to ShellServer.
pub struct ShellServiceImpl {
    server: Arc<ShellServer>,
    commit_window: Arc<RwLock<CommitWindowManager>>,
}

impl ShellServiceImpl {
    pub fn new(server: Arc<ShellServer>, commit_window: Arc<RwLock<CommitWindowManager>>) -> Self {
        Self { server, commit_window }
    }
}

/// Extract the authorization header from gRPC request metadata.
fn extract_auth<T>(request: &Request<T>) -> Result<String, Status> {
    request
        .metadata()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .ok_or_else(|| Status::unauthenticated("missing authorization header"))
}

#[tonic::async_trait]
impl ShellService for ShellServiceImpl {
    type ExecStream = ReceiverStream<Result<ExecOutput, Status>>;

    /// Execute a single command — auth, whitelist, fork/exec, stream output.
    async fn exec(
        &self,
        request: Request<ExecRequest>,
    ) -> Result<Response<Self::ExecStream>, Status> {
        let auth_header = extract_auth(&request)?;
        let req = request.into_inner();

        let (tx, rx) = tokio::sync::mpsc::channel(4);

        let server = self.server.clone();
        tokio::spawn(async move {
            match server.exec(&auth_header, &req.command, &req.args).await {
                Ok((result, _state_changing)) => {
                    // Send stdout
                    if !result.stdout.is_empty() {
                        let _ = tx
                            .send(Ok(ExecOutput {
                                output: Some(exec_output::Output::Stdout(result.stdout)),
                            }))
                            .await;
                    }
                    // Send stderr
                    if !result.stderr.is_empty() {
                        let _ = tx
                            .send(Ok(ExecOutput {
                                output: Some(exec_output::Output::Stderr(result.stderr)),
                            }))
                            .await;
                    }
                    // Send exit code
                    let _ = tx
                        .send(Ok(ExecOutput {
                            output: Some(exec_output::Output::ExitCode(result.exit_code)),
                        }))
                        .await;
                }
                Err(e) => {
                    let _ = tx
                        .send(Ok(ExecOutput {
                            output: Some(exec_output::Output::Error(e.to_string())),
                        }))
                        .await;
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    type ShellStream = ReceiverStream<Result<ShellOutput, Status>>;

    /// Interactive shell session — bidirectional streaming.
    ///
    /// Protocol:
    /// 1. Client sends `ShellOpen` with terminal dimensions
    /// 2. Server allocates PTY, returns `session_id`
    /// 3. Bidirectional: client sends stdin/resize/close, server sends stdout
    /// 4. On shell exit, server sends `exit_code` and ends stream
    #[allow(clippy::too_many_lines)]
    async fn shell(
        &self,
        request: Request<Streaming<ShellInput>>,
    ) -> Result<Response<Self::ShellStream>, Status> {
        use tokio_stream::StreamExt as _;

        // 1. Auth: extract and validate bearer token
        let auth_header = extract_auth(&request)?;
        let identity = self
            .server
            .authenticate(&auth_header)
            .map_err(|e| Status::unauthenticated(e.to_string()))?;

        // Shell requires ops role (not viewer)
        let vcluster_id = self.server.vcluster_id().to_string();
        if !has_ops_role(&identity, &vcluster_id) {
            return Err(Status::permission_denied(format!(
                "shell requires pact-ops-{vcluster_id} or pact-platform-admin role"
            )));
        }

        let mut in_stream = request.into_inner();

        // 2. Wait for the first message — must be ShellOpen
        let first_msg = in_stream
            .next()
            .await
            .ok_or_else(|| Status::invalid_argument("stream closed before ShellOpen"))?
            .map_err(|e| Status::internal(format!("stream error: {e}")))?;

        let open = match first_msg.input {
            Some(shell_input::Input::Open(open)) => open,
            _ => {
                return Err(Status::invalid_argument(
                    "first message must be ShellOpen with terminal dimensions",
                ));
            }
        };

        let rows = open.rows.min(500) as u16;
        let cols = open.cols.min(500) as u16;
        let term = if open.term.is_empty() { "xterm-256color".to_string() } else { open.term };

        // 3. Create session
        let node_id = self.server.node_id().to_string();
        let session_id = {
            let mut sessions = self.server.sessions().lock().await;
            let session = sessions
                .create_session(identity.clone(), node_id, vcluster_id, rows, cols, term)
                .map_err(|e| Status::resource_exhausted(e.to_string()))?;
            session.session_id.clone()
        };

        info!(
            session_id = %session_id,
            user = %identity.principal,
            "Shell session created, allocating PTY"
        );

        // 4. Allocate PTY (Linux-only)
        #[cfg(not(target_os = "linux"))]
        {
            // Clean up session on non-Linux
            let mut sessions = self.server.sessions().lock().await;
            sessions.remove(&session_id);
            return Err(Status::unimplemented("interactive shell requires Linux PTY support"));
        }

        #[cfg(target_os = "linux")]
        {
            use super::session::{allocate_pty, cleanup_session_bin_dir};

            // Get session data for PTY allocation
            let session_data = {
                let sessions = self.server.sessions().lock().await;
                sessions.get(&session_id).cloned()
            };
            let session_data = session_data.ok_or_else(|| Status::internal("session vanished"))?;

            let pty_handle = match allocate_pty(&session_data) {
                Ok(h) => h,
                Err(e) => {
                    let mut sessions = self.server.sessions().lock().await;
                    sessions.remove(&session_id);
                    return Err(Status::internal(format!("PTY allocation failed: {e}")));
                }
            };

            // Activate session
            {
                let mut sessions = self.server.sessions().lock().await;
                if let Some(s) = sessions.get_mut(&session_id) {
                    s.activate();
                }
            }

            // Create the output channel
            let (tx, rx) = tokio::sync::mpsc::channel::<Result<ShellOutput, Status>>(32);

            // Send session_id to client
            let _ = tx
                .send(Ok(ShellOutput {
                    output: Some(shell_output::Output::SessionId(session_id.clone())),
                }))
                .await;

            // 5. Bidirectional streaming
            // We need shared ownership of the PTY handle for read and write sides
            let pty = Arc::new(pty_handle);
            let sessions = self.server.sessions().clone();
            let sid = session_id.clone();

            tokio::spawn(async move {
                // Spawn a reader task: PTY master → client stdout
                let pty_reader = pty.clone();
                let tx_reader = tx.clone();
                let reader_handle = tokio::spawn(async move {
                    loop {
                        let pty_ref = pty_reader.clone();
                        let read_result = tokio::task::spawn_blocking(move || {
                            let mut buf = [0u8; 4096];
                            pty_ref.read(&mut buf).map(|n| buf[..n].to_vec())
                        })
                        .await;

                        match read_result {
                            Ok(Ok(data)) if !data.is_empty() => {
                                if tx_reader
                                    .send(Ok(ShellOutput {
                                        output: Some(shell_output::Output::Stdout(data)),
                                    }))
                                    .await
                                    .is_err()
                                {
                                    break; // Client disconnected
                                }
                            }
                            _ => break, // PTY closed or error
                        }
                    }
                });

                // Writer loop: client stdin → PTY master
                while let Some(msg) = in_stream.next().await {
                    match msg {
                        Ok(input) => match input.input {
                            Some(shell_input::Input::Stdin(data)) => {
                                if let Err(e) = pty.write(&data) {
                                    warn!(error = %e, "Failed to write to PTY");
                                    break;
                                }
                            }
                            Some(shell_input::Input::Resize(resize)) => {
                                let rows = resize.rows.min(500) as u16;
                                let cols = resize.cols.min(500) as u16;
                                if let Err(e) = pty.resize(rows, cols) {
                                    warn!(error = %e, "Failed to resize PTY");
                                }
                            }
                            Some(shell_input::Input::Close(_)) => {
                                info!(session_id = %sid, "Client requested shell close");
                                break;
                            }
                            Some(shell_input::Input::Open(_)) => {
                                // Ignore duplicate open messages
                            }
                            None => {}
                        },
                        Err(e) => {
                            warn!(error = %e, "Stream error from client");
                            break;
                        }
                    }
                }

                // 6. Cleanup: abort reader, close PTY, send exit code
                reader_handle.abort();
                let _ = reader_handle.await;

                // Try to get exit code from child
                // PtyHandle::close() kills the child. We extract the Arc to get owned value.
                // Since close() consumes self, we need to unwrap the Arc.
                let exit_code = match Arc::try_unwrap(pty) {
                    Ok(handle) => {
                        let _ = handle.close();
                        0 // close kills the process, exit code 0 for clean close
                    }
                    Err(_arc_pty) => {
                        // Other references still exist (shouldn't happen after abort)
                        0
                    }
                };

                let _ = tx
                    .send(Ok(ShellOutput {
                        output: Some(shell_output::Output::ExitCode(exit_code)),
                    }))
                    .await;

                // 7. Remove session from manager
                let mut sessions = sessions.lock().await;
                if let Some(s) = sessions.get_mut(&sid) {
                    s.close();
                    s.finalize();
                }
                sessions.remove(&sid);
                cleanup_session_bin_dir(&session_data);

                info!(session_id = %sid, "Shell session ended");
            });

            Ok(Response::new(ReceiverStream::new(rx)))
        }
    }

    /// List whitelisted commands for this node's vCluster.
    async fn list_commands(
        &self,
        request: Request<ListCommandsRequest>,
    ) -> Result<Response<ListCommandsResponse>, Status> {
        let _auth = extract_auth(&request)?;
        let commands = self.server.list_commands().await;
        let entries = commands
            .into_iter()
            .map(|entry| CommandEntry {
                command: entry.command,
                state_changing: entry.state_changing,
                description: entry.description,
            })
            .collect();

        Ok(Response::new(ListCommandsResponse { commands: entries }))
    }

    /// Extend the commit window by additional minutes.
    async fn extend_commit_window(
        &self,
        request: Request<ExtendWindowRequest>,
    ) -> Result<Response<ExtendWindowResponse>, Status> {
        let _auth = extract_auth(&request)?;
        let mins = request.into_inner().additional_minutes;
        if mins == 0 {
            return Ok(Response::new(ExtendWindowResponse {
                success: false,
                new_deadline_seconds: 0,
                error: Some("additional_minutes must be > 0".to_string()),
            }));
        }

        let mut cw = self.commit_window.write().await;
        cw.extend(mins * 60);

        // Calculate seconds until deadline
        let deadline_secs = cw.seconds_remaining();

        Ok(Response::new(ExtendWindowResponse {
            success: true,
            new_deadline_seconds: deadline_secs,
            error: None,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::auth::AuthConfig;
    use crate::shell::exec::ExecConfig;
    use pact_common::config::CommitWindowConfig;

    const TEST_SECRET: &[u8] = b"test-secret-key-for-pact-development";

    fn test_commit_window() -> Arc<RwLock<CommitWindowManager>> {
        Arc::new(RwLock::new(CommitWindowManager::new(CommitWindowConfig::default())))
    }

    fn test_shell_server() -> Arc<ShellServer> {
        Arc::new(ShellServer::new(
            AuthConfig {
                issuer: "https://auth.test.example.com".into(),
                audience: "pact-agent".into(),
                hmac_secret: Some(TEST_SECRET.to_vec()),
                jwks_url: None,
            },
            ExecConfig::default(),
            "node-001".into(),
            "ml-training".into(),
            true,
            10,
        ))
    }

    fn make_token(sub: &str, role: &str) -> String {
        use crate::shell::auth::TokenClaims;
        use jsonwebtoken::{encode, EncodingKey, Header};

        let claims = TokenClaims {
            sub: sub.into(),
            aud: crate::shell::auth::StringOrVec::Single("pact-agent".into()),
            iss: "https://auth.test.example.com".into(),
            exp: (chrono::Utc::now().timestamp() + 3600) as u64,
            iat: chrono::Utc::now().timestamp() as u64,
            pact_role: Some(role.into()),
            pact_principal_type: None,
        };
        encode(&Header::default(), &claims, &EncodingKey::from_secret(TEST_SECRET)).unwrap()
    }

    #[tokio::test]
    async fn exec_returns_output_stream() {
        let server = test_shell_server();
        let svc = ShellServiceImpl::new(server, test_commit_window());

        let token = make_token("ops@example.com", "pact-ops-ml-training");
        let mut request =
            Request::new(ExecRequest { command: "echo".into(), args: vec!["hello".into()] });
        request.metadata_mut().insert("authorization", format!("Bearer {token}").parse().unwrap());

        let resp = svc.exec(request).await.unwrap();
        let mut stream = resp.into_inner();

        let mut got_stdout = false;
        let mut got_exit = false;
        while let Some(Ok(output)) = tokio_stream::StreamExt::next(&mut stream).await {
            match output.output {
                Some(exec_output::Output::Stdout(data)) => {
                    assert!(String::from_utf8_lossy(&data).contains("hello"));
                    got_stdout = true;
                }
                Some(exec_output::Output::ExitCode(code)) => {
                    assert_eq!(code, 0);
                    got_exit = true;
                }
                _ => {}
            }
        }
        assert!(got_stdout, "should have received stdout");
        assert!(got_exit, "should have received exit code");
    }

    #[tokio::test]
    async fn exec_without_auth_fails() {
        let server = test_shell_server();
        let svc = ShellServiceImpl::new(server, test_commit_window());

        let request = Request::new(ExecRequest { command: "echo".into(), args: vec![] });

        let result = svc.exec(request).await;
        assert_eq!(result.unwrap_err().code(), tonic::Code::Unauthenticated);
    }

    #[tokio::test]
    async fn list_commands_returns_entries() {
        let server = test_shell_server();
        let svc = ShellServiceImpl::new(server, test_commit_window());

        let token = make_token("ops@example.com", "pact-ops-ml-training");
        let mut request = Request::new(ListCommandsRequest {});
        request.metadata_mut().insert("authorization", format!("Bearer {token}").parse().unwrap());

        let resp = svc.list_commands(request).await.unwrap();
        let commands = resp.into_inner().commands;
        assert!(!commands.is_empty());

        // Should include well-known commands
        let names: Vec<&str> = commands.iter().map(|c| c.command.as_str()).collect();
        assert!(names.contains(&"ps"), "should include 'ps'");
        assert!(names.contains(&"nvidia-smi"), "should include 'nvidia-smi'");
    }

    #[tokio::test]
    async fn list_commands_without_auth_fails() {
        let server = test_shell_server();
        let svc = ShellServiceImpl::new(server, test_commit_window());

        let request = Request::new(ListCommandsRequest {});
        let result = svc.list_commands(request).await;
        assert_eq!(result.unwrap_err().code(), tonic::Code::Unauthenticated);
    }

    #[tokio::test]
    async fn extend_commit_window_without_auth_fails() {
        let server = test_shell_server();
        let svc = ShellServiceImpl::new(server, test_commit_window());

        let request = Request::new(ExtendWindowRequest { additional_minutes: 5 });
        let result = svc.extend_commit_window(request).await;
        assert_eq!(result.unwrap_err().code(), tonic::Code::Unauthenticated);
    }

    /// Verify that auth extraction fails without authorization header.
    /// Streaming<ShellInput> is not constructible in unit tests (tonic internals),
    /// so we verify the auth helper that shell() uses.
    #[test]
    fn shell_auth_extraction_requires_header() {
        let empty_request: Request<()> = Request::new(());
        let result = extract_auth(&empty_request);
        assert_eq!(result.unwrap_err().code(), tonic::Code::Unauthenticated);
    }

    /// Verify that auth extraction succeeds with a valid header.
    #[test]
    fn shell_auth_extraction_with_header() {
        let token = make_token("ops@example.com", "pact-ops-ml-training");
        let mut request: Request<()> = Request::new(());
        request.metadata_mut().insert("authorization", format!("Bearer {token}").parse().unwrap());

        let result = extract_auth(&request);
        assert!(result.is_ok());
        assert!(result.unwrap().starts_with("Bearer "));
    }

    /// Verify that node_id and vcluster_id accessors work (used by shell).
    #[test]
    fn shell_server_accessors() {
        let server = test_shell_server();
        assert_eq!(server.node_id(), "node-001");
        assert_eq!(server.vcluster_id(), "ml-training");
    }

    /// Verify that shell auth requires ops role (not viewer).
    #[test]
    fn shell_requires_ops_role() {
        use crate::shell::auth::has_ops_role;
        use pact_common::types::{Identity, PrincipalType};

        let ops = Identity {
            principal: "ops@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-ops-ml-training".into(),
        };
        assert!(has_ops_role(&ops, "ml-training"));

        let viewer = Identity {
            principal: "viewer@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-viewer-ml-training".into(),
        };
        assert!(!has_ops_role(&viewer, "ml-training"));

        let admin = Identity {
            principal: "admin@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-platform-admin".into(),
        };
        assert!(has_ops_role(&admin, "ml-training"));
    }

    /// On non-Linux, shell should return Unimplemented with a clear message.
    /// On Linux, shell should proceed past auth (tested via integration).
    #[cfg(not(target_os = "linux"))]
    #[tokio::test]
    async fn shell_returns_unimplemented_on_non_linux() {
        // We can't construct a real Streaming<ShellInput> in unit tests,
        // but we can verify the session/PTY path returns the right error
        // by checking allocate_pty directly.
        use crate::shell::session::{allocate_pty, ShellSession};
        use pact_common::types::{Identity, PrincipalType};

        let session = ShellSession::new(
            Identity {
                principal: "admin@example.com".into(),
                principal_type: PrincipalType::Human,
                role: "pact-ops-ml-training".into(),
            },
            "node-001".into(),
            "ml-training".into(),
            24,
            80,
            "xterm-256color".into(),
        );
        let result = allocate_pty(&session);
        assert!(result.is_err(), "allocate_pty should fail on non-Linux");
    }
}
