//! Single command execution endpoint.
//!
//! `pact exec <node> -- <command> [args]`
//!
//! Flow:
//! 1. Authenticate (OIDC token from gRPC metadata)
//! 2. Authorize (whitelist check → policy evaluation)
//! 3. Classify (read-only vs state-changing → commit window)
//! 4. Execute (fork/exec, NO shell interpretation)
//! 5. Stream stdout/stderr back
//! 6. Log to journal (ExecLog entry)

use std::process::Stdio;

use tokio::io::BufReader;
use tokio::process::Command;
use tracing::debug;

use pact_common::types::Identity;

/// Result of a command execution.
#[derive(Debug, Clone)]
pub struct ExecResult {
    /// Combined stdout output.
    pub stdout: Vec<u8>,
    /// Combined stderr output.
    pub stderr: Vec<u8>,
    /// Process exit code.
    pub exit_code: i32,
}

/// Errors during command execution.
#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error("command not whitelisted: {0}")]
    NotWhitelisted(String),
    #[error("command not found on PATH: {0}")]
    NotFound(String),
    #[error("execution failed: {0}")]
    ExecutionFailed(String),
    #[error("command timed out after {0} seconds")]
    Timeout(u64),
}

/// Configuration for command execution.
#[derive(Debug, Clone)]
pub struct ExecConfig {
    /// Maximum execution time in seconds (0 = no limit).
    pub timeout_seconds: u64,
    /// Maximum output size in bytes (to prevent runaway output).
    pub max_output_bytes: usize,
}

impl Default for ExecConfig {
    fn default() -> Self {
        Self {
            timeout_seconds: 300,         // 5 minutes
            max_output_bytes: 10_485_760, // 10 MB
        }
    }
}

/// Execute a command directly via fork/exec (no shell interpretation).
///
/// This is the core execution function. The command is executed directly
/// as a child process — not passed to a shell. This prevents shell injection.
pub async fn execute_command(
    command: &str,
    args: &[String],
    config: &ExecConfig,
) -> Result<ExecResult, ExecError> {
    debug!(command, ?args, "Executing command");

    let mut child = Command::new(command)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Clear environment for security — only pass safe vars
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", "/tmp")
        .env("LANG", "C.UTF-8")
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ExecError::NotFound(command.into())
            } else {
                ExecError::ExecutionFailed(e.to_string())
            }
        })?;

    let stdout_handle = child.stdout.take();
    let stderr_handle = child.stderr.take();

    // Read stdout
    let max_bytes = config.max_output_bytes;
    let stdout_task = tokio::spawn(async move {
        let mut output = Vec::new();
        if let Some(stdout) = stdout_handle {
            let mut reader = BufReader::new(stdout);
            let mut buf = vec![0u8; 8192];
            loop {
                match tokio::io::AsyncReadExt::read(&mut reader, &mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        if output.len() + n > max_bytes {
                            output.extend_from_slice(&buf[..max_bytes - output.len()]);
                            break;
                        }
                        output.extend_from_slice(&buf[..n]);
                    }
                    Err(_) => break,
                }
            }
        }
        output
    });

    // Read stderr
    let stderr_task = tokio::spawn(async move {
        let mut output = Vec::new();
        if let Some(stderr) = stderr_handle {
            let mut reader = BufReader::new(stderr);
            let mut buf = vec![0u8; 8192];
            loop {
                match tokio::io::AsyncReadExt::read(&mut reader, &mut buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        if output.len() + n > max_bytes {
                            output.extend_from_slice(&buf[..max_bytes - output.len()]);
                            break;
                        }
                        output.extend_from_slice(&buf[..n]);
                    }
                    Err(_) => break,
                }
            }
        }
        output
    });

    // Wait for completion with timeout
    let result = if config.timeout_seconds > 0 {
        match tokio::time::timeout(
            std::time::Duration::from_secs(config.timeout_seconds),
            child.wait(),
        )
        .await
        {
            Ok(Ok(status)) => status,
            Ok(Err(e)) => return Err(ExecError::ExecutionFailed(e.to_string())),
            Err(_) => {
                // Kill the process on timeout
                let _ = child.kill().await;
                return Err(ExecError::Timeout(config.timeout_seconds));
            }
        }
    } else {
        child.wait().await.map_err(|e| ExecError::ExecutionFailed(e.to_string()))?
    };

    let stdout = stdout_task.await.unwrap_or_default();
    let stderr = stderr_task.await.unwrap_or_default();

    Ok(ExecResult { stdout, stderr, exit_code: result.code().unwrap_or(-1) })
}

/// Audit log entry for a command execution.
#[derive(Debug, Clone)]
pub struct ExecAuditEntry {
    pub command: String,
    pub args: Vec<String>,
    pub actor: Identity,
    pub node_id: String,
    pub vcluster_id: String,
    pub exit_code: i32,
    pub state_changing: bool,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn execute_echo_command() {
        let result =
            execute_command("echo", &["hello".into(), "world".into()], &ExecConfig::default())
                .await
                .unwrap();

        assert_eq!(result.exit_code, 0);
        let stdout = String::from_utf8_lossy(&result.stdout);
        assert!(stdout.contains("hello world"));
    }

    #[tokio::test]
    async fn execute_command_captures_stderr() {
        // ls on nonexistent path writes to stderr
        let result = execute_command(
            "ls",
            &["/nonexistent/path/that/does/not/exist".into()],
            &ExecConfig::default(),
        )
        .await
        .unwrap();

        assert_ne!(result.exit_code, 0);
        assert!(!result.stderr.is_empty());
    }

    #[tokio::test]
    async fn execute_command_not_found() {
        let result = execute_command("nonexistent-binary-12345", &[], &ExecConfig::default()).await;

        assert!(matches!(result, Err(ExecError::NotFound(_))));
    }

    #[tokio::test]
    async fn execute_command_with_exit_code() {
        let result = execute_command("false", &[], &ExecConfig::default()).await.unwrap();
        assert_ne!(result.exit_code, 0);

        let result = execute_command("true", &[], &ExecConfig::default()).await.unwrap();
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn execute_command_timeout() {
        let config = ExecConfig { timeout_seconds: 1, ..ExecConfig::default() };
        let result = execute_command("sleep", &["30".into()], &config).await;
        assert!(matches!(result, Err(ExecError::Timeout(1))));
    }

    #[tokio::test]
    async fn execute_no_shell_interpretation() {
        // Passing shell metacharacters should NOT be interpreted
        // echo should receive the literal string "$(whoami)" as an argument
        let result =
            execute_command("echo", &["$(whoami)".into()], &ExecConfig::default()).await.unwrap();

        let stdout = String::from_utf8_lossy(&result.stdout);
        // The literal string should appear, not the result of whoami
        assert!(
            stdout.contains("$(whoami)"),
            "shell metachar should not be interpreted, got: {stdout}"
        );
    }

    #[tokio::test]
    async fn execute_with_clean_environment() {
        // env should show minimal environment
        let result = execute_command("env", &[], &ExecConfig::default()).await.unwrap();

        let stdout = String::from_utf8_lossy(&result.stdout);
        // Should have our restricted PATH
        assert!(stdout.contains("PATH="));
        // Should not have user's HOME or sensitive vars
        assert!(!stdout.contains("SSH_"), "SSH vars should not leak");
    }

    #[test]
    fn exec_config_defaults() {
        let config = ExecConfig::default();
        assert_eq!(config.timeout_seconds, 300);
        assert_eq!(config.max_output_bytes, 10_485_760);
    }
}
