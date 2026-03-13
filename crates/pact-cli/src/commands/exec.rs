//! `pact exec` — run a command on a remote node.

/// Result of a remote exec operation.
#[derive(Debug, Clone)]
pub struct ExecResult {
    pub node_id: String,
    pub command: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Exit codes per cli-design.md.
pub mod exit_codes {
    pub const SUCCESS: i32 = 0;
    pub const GENERAL_ERROR: i32 = 1;
    pub const AUTH_FAILURE: i32 = 2;
    pub const POLICY_REJECTION: i32 = 3;
    pub const CONFLICT: i32 = 4;
    pub const TIMEOUT: i32 = 5;
    pub const NOT_WHITELISTED: i32 = 6;
    pub const ROLLBACK_FAILED: i32 = 10;
}

/// Format exec result for display.
pub fn format_exec_result(result: &ExecResult) -> String {
    let mut output = String::new();

    if !result.stdout.is_empty() {
        output.push_str(&result.stdout);
    }

    if !result.stderr.is_empty() {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(&result.stderr);
    }

    output
}

/// Map exec errors to CLI exit codes.
pub fn error_to_exit_code(error: &str) -> i32 {
    if error.contains("not whitelisted") || error.contains("not allowed") {
        exit_codes::NOT_WHITELISTED
    } else if error.contains("auth") || error.contains("token") || error.contains("unauthorized") {
        exit_codes::AUTH_FAILURE
    } else if error.contains("policy") || error.contains("denied") {
        exit_codes::POLICY_REJECTION
    } else if error.contains("timeout") || error.contains("unreachable") {
        exit_codes::TIMEOUT
    } else {
        exit_codes::GENERAL_ERROR
    }
}

/// Parse a command string into (command, args) for the exec request.
pub fn parse_exec_command(parts: &[String]) -> Result<(String, Vec<String>), String> {
    if parts.is_empty() {
        return Err("No command specified".into());
    }

    let command = parts[0].clone();
    let args = parts[1..].to_vec();
    Ok((command, args))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_exec_stdout_only() {
        let result = ExecResult {
            node_id: "node042".into(),
            command: "hostname".into(),
            stdout: "node042.cluster".into(),
            stderr: String::new(),
            exit_code: 0,
        };
        let output = format_exec_result(&result);
        assert_eq!(output, "node042.cluster");
    }

    #[test]
    fn format_exec_stderr_only() {
        let result = ExecResult {
            node_id: "node042".into(),
            command: "ls /nonexistent".into(),
            stdout: String::new(),
            stderr: "ls: cannot access '/nonexistent': No such file or directory".into(),
            exit_code: 2,
        };
        let output = format_exec_result(&result);
        assert!(output.contains("No such file or directory"));
    }

    #[test]
    fn format_exec_both_streams() {
        let result = ExecResult {
            node_id: "node042".into(),
            command: "nvidia-smi".into(),
            stdout: "GPU 0: A100".into(),
            stderr: "Warning: persistence mode disabled".into(),
            exit_code: 0,
        };
        let output = format_exec_result(&result);
        assert!(output.contains("GPU 0: A100"));
        assert!(output.contains("Warning:"));
    }

    #[test]
    fn error_to_exit_code_mappings() {
        assert_eq!(error_to_exit_code("command not whitelisted"), exit_codes::NOT_WHITELISTED);
        assert_eq!(error_to_exit_code("auth token expired"), exit_codes::AUTH_FAILURE);
        assert_eq!(error_to_exit_code("policy denied"), exit_codes::POLICY_REJECTION);
        assert_eq!(error_to_exit_code("connection timeout"), exit_codes::TIMEOUT);
        assert_eq!(error_to_exit_code("something else"), exit_codes::GENERAL_ERROR);
    }

    #[test]
    fn parse_exec_command_simple() {
        let parts = vec!["hostname".to_string()];
        let (cmd, args) = parse_exec_command(&parts).unwrap();
        assert_eq!(cmd, "hostname");
        assert!(args.is_empty());
    }

    #[test]
    fn parse_exec_command_with_args() {
        let parts =
            vec!["nvidia-smi".to_string(), "-q".to_string(), "-d".to_string(), "ECC".to_string()];
        let (cmd, args) = parse_exec_command(&parts).unwrap();
        assert_eq!(cmd, "nvidia-smi");
        assert_eq!(args, vec!["-q", "-d", "ECC"]);
    }

    #[test]
    fn parse_exec_command_empty_fails() {
        let result = parse_exec_command(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn exit_codes_correct_values() {
        assert_eq!(exit_codes::SUCCESS, 0);
        assert_eq!(exit_codes::AUTH_FAILURE, 2);
        assert_eq!(exit_codes::POLICY_REJECTION, 3);
        assert_eq!(exit_codes::NOT_WHITELISTED, 6);
        assert_eq!(exit_codes::ROLLBACK_FAILED, 10);
    }
}
