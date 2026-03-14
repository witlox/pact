//! ShellService gRPC adapter — wraps the ShellServer for tonic.
//!
//! Maps proto ShellService RPCs to ShellServer methods.
//! Auth token extracted from gRPC metadata (authorization header).

use std::sync::Arc;

use tokio::sync::RwLock;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};
use tracing::warn;

use pact_common::proto::shell::{
    exec_output, shell_service_server::ShellService, CommandEntry, ExecOutput, ExecRequest,
    ExtendWindowRequest, ExtendWindowResponse, ListCommandsRequest, ListCommandsResponse,
    ShellInput, ShellOutput,
};

use crate::commit::CommitWindowManager;

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
    /// Not yet implemented (Phase 3.3: PTY allocation + rbash).
    async fn shell(
        &self,
        _request: Request<Streaming<ShellInput>>,
    ) -> Result<Response<Self::ShellStream>, Status> {
        warn!("Interactive shell not yet implemented");
        Err(Status::unimplemented("interactive shell requires PTY support (Phase 3.3)"))
    }

    /// List whitelisted commands for this node's vCluster.
    async fn list_commands(
        &self,
        _request: Request<ListCommandsRequest>,
    ) -> Result<Response<ListCommandsResponse>, Status> {
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

        let resp = svc.list_commands(Request::new(ListCommandsRequest {})).await.unwrap();
        let commands = resp.into_inner().commands;
        assert!(!commands.is_empty());

        // Should include well-known commands
        let names: Vec<&str> = commands.iter().map(|c| c.command.as_str()).collect();
        assert!(names.contains(&"ps"), "should include 'ps'");
        assert!(names.contains(&"nvidia-smi"), "should include 'nvidia-smi'");
    }

    // Note: shell() test omitted — Streaming<ShellInput> is not constructible
    // from test code without a real tonic transport. The method returns
    // Unimplemented, which is verified by e2e tests.
}
