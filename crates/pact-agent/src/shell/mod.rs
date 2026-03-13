//! Shell server — exec and interactive shell endpoints.
//!
//! Two execution modes (ADR-007):
//! - `pact exec`: single command, fork/exec'd directly, no shell interpretation
//! - `pact shell`: interactive session via restricted bash (rbash) + PTY
//!
//! Security model:
//! - Whitelist enforcement via PATH restriction (not command parsing)
//! - Shell does NOT pre-classify commands (invariant S6)
//! - Drift observer detects actual state changes post-execution
//! - PROMPT_COMMAND logs every command for audit (invariant S4)
//!
//! Cross-platform:
//! - Linux: real PTY allocation, symlink-based PATH restriction
//! - macOS: stubs compile, MockShellSession for development

pub mod auth;
pub mod exec;
pub mod session;
pub mod whitelist;

use std::sync::Arc;

use tokio::sync::{Mutex, RwLock};
use tracing::info;

use pact_common::types::Identity;

use crate::shell::auth::{
    extract_bearer_token, has_ops_role, has_viewer_role, is_platform_admin, validate_token,
    AuthConfig, AuthError,
};
use crate::shell::exec::{execute_command, ExecConfig, ExecResult};
use crate::shell::session::SessionManager;
use crate::shell::whitelist::WhitelistManager;

/// Shell server state shared across gRPC handlers.
pub struct ShellServer {
    /// Whitelist manager.
    whitelist: Arc<RwLock<WhitelistManager>>,
    /// Session manager.
    sessions: Arc<Mutex<SessionManager>>,
    /// Auth configuration.
    auth_config: AuthConfig,
    /// Exec configuration.
    exec_config: ExecConfig,
    /// Node ID this server runs on.
    node_id: String,
    /// vCluster this node belongs to.
    vcluster_id: String,
}

impl ShellServer {
    pub fn new(
        auth_config: AuthConfig,
        exec_config: ExecConfig,
        node_id: String,
        vcluster_id: String,
        learning_mode: bool,
        max_sessions: usize,
    ) -> Self {
        Self {
            whitelist: Arc::new(RwLock::new(WhitelistManager::new(learning_mode))),
            sessions: Arc::new(Mutex::new(SessionManager::new(max_sessions))),
            auth_config,
            exec_config,
            node_id,
            vcluster_id,
        }
    }

    /// Authenticate a request from gRPC metadata.
    pub fn authenticate(&self, auth_header: &str) -> Result<Identity, AuthError> {
        let token = extract_bearer_token(auth_header).ok_or(AuthError::InvalidFormat)?;
        let claims = validate_token(token, &self.auth_config)?;
        Ok(auth::claims_to_identity(&claims))
    }

    /// Authorize an exec command.
    ///
    /// Flow:
    /// 1. Check if command is whitelisted (S1)
    /// 2. Platform admin can bypass whitelist (S2)
    /// 3. Check if identity has ops role for this vCluster
    pub async fn authorize_exec(
        &self,
        identity: &Identity,
        command: &str,
    ) -> Result<bool, AuthError> {
        let whitelist = self.whitelist.read().await;

        // S1: whitelist check
        let is_whitelisted = whitelist.is_exec_allowed(command);

        if !is_whitelisted {
            // S2: platform admin bypass
            if is_platform_admin(identity) {
                info!(
                    user = %identity.principal,
                    command,
                    "Platform admin executing non-whitelisted command (S2 bypass)"
                );
                return Ok(true);
            }

            // Record denied command in learning mode
            drop(whitelist);
            self.whitelist.write().await.record_denied(command);
            return Err(AuthError::InsufficientPrivileges(format!(
                "command not whitelisted: {command}"
            )));
        }

        let state_changing = whitelist.is_state_changing(command);

        // Check role — viewers can exec read-only commands only
        if !has_ops_role(identity, &self.vcluster_id) {
            if has_viewer_role(identity, &self.vcluster_id) && !state_changing {
                return Ok(false);
            }
            return Err(AuthError::InsufficientPrivileges(format!(
                "requires pact-ops-{} or pact-platform-admin role",
                self.vcluster_id
            )));
        }

        Ok(state_changing)
    }

    /// Execute a single command (the full pipeline).
    ///
    /// Auth → whitelist → classify → execute → audit.
    pub async fn exec(
        &self,
        auth_header: &str,
        command: &str,
        args: &[String],
    ) -> Result<(ExecResult, bool), ShellError> {
        // 1. Authenticate
        let identity = self.authenticate(auth_header).map_err(ShellError::Auth)?;

        // 2. Authorize (returns whether command is state-changing)
        let state_changing =
            self.authorize_exec(&identity, command).await.map_err(ShellError::Auth)?;

        info!(
            user = %identity.principal,
            node = %self.node_id,
            command,
            state_changing,
            "Executing command"
        );

        // 3. Execute
        let result =
            execute_command(command, args, &self.exec_config).await.map_err(ShellError::Exec)?;

        // 4. TODO: Log to journal (ExecLog entry)
        // 5. TODO: If state_changing, interact with commit window

        Ok((result, state_changing))
    }

    /// Get the whitelist manager (for external updates from config subscription).
    pub fn whitelist(&self) -> &Arc<RwLock<WhitelistManager>> {
        &self.whitelist
    }

    /// Get the session manager.
    pub fn sessions(&self) -> &Arc<Mutex<SessionManager>> {
        &self.sessions
    }

    /// List available commands.
    pub async fn list_commands(&self) -> Vec<whitelist::WhitelistEntry> {
        let wl = self.whitelist.read().await;
        wl.exec_commands().into_iter().cloned().collect()
    }
}

/// Shell server errors.
#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    #[error("authentication failed: {0}")]
    Auth(#[from] AuthError),
    #[error("execution failed: {0}")]
    Exec(#[from] exec::ExecError),
    #[error("session error: {0}")]
    Session(#[from] session::SessionError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};

    const TEST_SECRET: &[u8] = b"test-secret-key-for-pact-development";
    const TEST_ISSUER: &str = "https://auth.test.example.com";
    const TEST_AUDIENCE: &str = "pact-agent";

    fn test_auth_config() -> AuthConfig {
        AuthConfig {
            issuer: TEST_ISSUER.into(),
            audience: TEST_AUDIENCE.into(),
            hmac_secret: Some(TEST_SECRET.to_vec()),
        }
    }

    fn test_server() -> ShellServer {
        ShellServer::new(
            test_auth_config(),
            ExecConfig::default(),
            "node-001".into(),
            "ml-training".into(),
            true, // learning mode
            10,   // max sessions
        )
    }

    fn make_token(sub: &str, role: &str) -> String {
        let claims = auth::TokenClaims {
            sub: sub.into(),
            aud: auth::StringOrVec::Single(TEST_AUDIENCE.into()),
            iss: TEST_ISSUER.into(),
            exp: (chrono::Utc::now().timestamp() + 3600) as u64,
            iat: chrono::Utc::now().timestamp() as u64,
            pact_role: Some(role.into()),
            pact_principal_type: None,
        };
        encode(&Header::default(), &claims, &EncodingKey::from_secret(TEST_SECRET)).unwrap()
    }

    #[test]
    fn authenticate_valid_token() {
        let server = test_server();
        let token = make_token("admin@example.com", "pact-platform-admin");
        let auth_header = format!("Bearer {token}");

        let identity = server.authenticate(&auth_header).unwrap();
        assert_eq!(identity.principal, "admin@example.com");
        assert_eq!(identity.role, "pact-platform-admin");
    }

    #[test]
    fn authenticate_missing_bearer_prefix() {
        let server = test_server();
        let result = server.authenticate("not-a-bearer-token");
        assert!(matches!(result, Err(AuthError::InvalidFormat)));
    }

    #[test]
    fn authenticate_invalid_token() {
        let server = test_server();
        let result = server.authenticate("Bearer invalid.jwt.token");
        assert!(matches!(result, Err(AuthError::InvalidToken(_))));
    }

    #[tokio::test]
    async fn authorize_whitelisted_command() {
        let server = test_server();
        let identity = Identity {
            principal: "ops@example.com".into(),
            principal_type: pact_common::types::PrincipalType::Human,
            role: "pact-ops-ml-training".into(),
        };

        let state_changing = server.authorize_exec(&identity, "ps").await.unwrap();
        assert!(!state_changing);
    }

    #[tokio::test]
    async fn authorize_state_changing_command() {
        let server = test_server();
        let identity = Identity {
            principal: "ops@example.com".into(),
            principal_type: pact_common::types::PrincipalType::Human,
            role: "pact-ops-ml-training".into(),
        };

        let state_changing = server.authorize_exec(&identity, "systemctl").await.unwrap();
        assert!(state_changing);
    }

    #[tokio::test]
    async fn authorize_non_whitelisted_denied() {
        let server = test_server();
        let identity = Identity {
            principal: "ops@example.com".into(),
            principal_type: pact_common::types::PrincipalType::Human,
            role: "pact-ops-ml-training".into(),
        };

        let result = server.authorize_exec(&identity, "vim").await;
        assert!(matches!(result, Err(AuthError::InsufficientPrivileges(_))));

        // Learning mode should have recorded the denied command
        let wl = server.whitelist.read().await;
        assert!(wl.denied_commands().contains(&"vim".to_string()));
    }

    #[tokio::test]
    async fn authorize_platform_admin_bypasses_whitelist() {
        let server = test_server();
        let admin = Identity {
            principal: "admin@example.com".into(),
            principal_type: pact_common::types::PrincipalType::Human,
            role: "pact-platform-admin".into(),
        };

        // vim is not whitelisted but platform admin can bypass (S2)
        let result = server.authorize_exec(&admin, "vim").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn authorize_wrong_vcluster_denied() {
        let server = test_server();
        let identity = Identity {
            principal: "ops@example.com".into(),
            principal_type: pact_common::types::PrincipalType::Human,
            role: "pact-ops-other-vcluster".into(),
        };

        let result = server.authorize_exec(&identity, "ps").await;
        assert!(matches!(result, Err(AuthError::InsufficientPrivileges(_))));
    }

    #[tokio::test]
    async fn viewer_can_exec_read_only() {
        let server = test_server();
        let viewer = Identity {
            principal: "viewer@example.com".into(),
            principal_type: pact_common::types::PrincipalType::Human,
            role: "pact-viewer-ml-training".into(),
        };

        let state_changing = server.authorize_exec(&viewer, "ps").await.unwrap();
        assert!(!state_changing);
    }

    #[tokio::test]
    async fn viewer_denied_state_changing() {
        let server = test_server();
        let viewer = Identity {
            principal: "viewer@example.com".into(),
            principal_type: pact_common::types::PrincipalType::Human,
            role: "pact-viewer-ml-training".into(),
        };

        let result = server.authorize_exec(&viewer, "systemctl").await;
        assert!(matches!(result, Err(AuthError::InsufficientPrivileges(_))));
    }

    #[tokio::test]
    async fn exec_full_pipeline() {
        let server = test_server();
        let token = make_token("ops@example.com", "pact-ops-ml-training");
        let auth_header = format!("Bearer {token}");

        let (result, state_changing) =
            server.exec(&auth_header, "echo", &["hello".into()]).await.unwrap();

        assert_eq!(result.exit_code, 0);
        assert!(!state_changing);
        let stdout = String::from_utf8_lossy(&result.stdout);
        assert!(stdout.contains("hello"));
    }

    #[tokio::test]
    async fn exec_auth_failure() {
        let server = test_server();
        let result = server.exec("Bearer bad.token", "echo", &[]).await;
        assert!(matches!(result, Err(ShellError::Auth(_))));
    }

    #[tokio::test]
    async fn exec_whitelist_failure() {
        let server = test_server();
        let token = make_token("ops@example.com", "pact-ops-ml-training");
        let auth_header = format!("Bearer {token}");

        let result = server.exec(&auth_header, "vim", &[]).await;
        assert!(matches!(result, Err(ShellError::Auth(AuthError::InsufficientPrivileges(_)))));
    }

    #[tokio::test]
    async fn list_commands_returns_sorted() {
        let server = test_server();
        let commands = server.list_commands().await;
        assert!(!commands.is_empty());

        let names: Vec<&str> = commands.iter().map(|e| e.command.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
    }
}
