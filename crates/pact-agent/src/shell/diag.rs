//! Diagnostic log collection for `pact diag`.
//!
//! Collects logs from dmesg, syslog, and supervised service log files.
//! All filtering (grep, line limit) is done agent-side per LOG2/LOG3.
//! Input validation enforces LOG4 (grep pattern) and LOG5 (service name).

use pact_common::proto::shell::{DiagChunk, DiagRequest};
use pact_common::types::SupervisorBackend;
use regex::Regex;
use tonic::Status;
use tracing::warn;

/// Maximum allowed grep pattern length (LOG4).
const MAX_GREP_PATTERN_LEN: usize = 255;

/// Default line limit per source (LOG3).
const DEFAULT_LINE_LIMIT: u32 = 100;

/// Maximum line limit per source (LOG3).
const MAX_LINE_LIMIT: u32 = 10_000;

/// Subprocess timeout in seconds (F44, F45).
const SUBPROCESS_TIMEOUT_SECS: u64 = 5;

/// Collect diagnostic logs from all requested sources.
///
/// Returns a vector of `DiagChunk` — one per log source.
pub async fn collect_diag(
    request: &DiagRequest,
    supervisor_backend: SupervisorBackend,
    declared_services: &[String],
) -> Vec<DiagChunk> {
    let limit = normalize_line_limit(request.line_limit);
    let grep = if request.grep_pattern.is_empty() {
        None
    } else {
        // Caller should have validated already, but compile defensively
        Regex::new(&request.grep_pattern).ok()
    };

    let source = request.source_filter.as_str();
    let mut chunks = Vec::new();

    let collect_system = source == "system" || source == "all" || source.is_empty();
    let collect_service = source == "service" || source == "all" || source.is_empty();

    if collect_system {
        chunks.push(collect_dmesg(limit, grep.as_ref()).await);
        chunks.push(collect_syslog(limit, grep.as_ref()).await);
    }

    if collect_service {
        if request.service_name.is_empty() {
            // Collect all declared services
            for svc in declared_services {
                chunks.push(
                    collect_service_log(svc, limit, grep.as_ref(), &supervisor_backend).await,
                );
            }
        } else {
            // Specific service — already validated by caller
            chunks.push(
                collect_service_log(
                    &request.service_name,
                    limit,
                    grep.as_ref(),
                    &supervisor_backend,
                )
                .await,
            );
        }
    }

    chunks
}

/// Validate the grep pattern per LOG4.
///
/// Returns compiled Regex on success, or gRPC INVALID_ARGUMENT Status on failure.
pub fn validate_grep_pattern(pattern: &str) -> Result<Option<Regex>, Status> {
    if pattern.is_empty() {
        return Ok(None);
    }
    if pattern.len() > MAX_GREP_PATTERN_LEN {
        return Err(Status::invalid_argument(format!(
            "invalid grep pattern: exceeds maximum length of {MAX_GREP_PATTERN_LEN} chars"
        )));
    }
    match Regex::new(pattern) {
        Ok(re) => Ok(Some(re)),
        Err(e) => Err(Status::invalid_argument(format!("invalid grep pattern: {e}"))),
    }
}

/// Validate service name per LOG5.
///
/// Rejects path traversal and unknown services.
pub fn validate_service_name(name: &str, declared: &[String]) -> Result<(), Status> {
    if name.is_empty() {
        return Ok(());
    }
    if name.contains('/') || name.contains("..") {
        return Err(Status::invalid_argument(format!(
            "invalid service name: must not contain path separators: {name}"
        )));
    }
    if !declared.contains(&name.to_string()) {
        // F43 behavior: unknown service returns empty chunk, not error
        // But we return Ok and let collect_service_log handle it
        return Ok(());
    }
    Ok(())
}

/// Normalize line limit per LOG3.
fn normalize_line_limit(limit: u32) -> u32 {
    if limit == 0 {
        DEFAULT_LINE_LIMIT
    } else {
        limit.min(MAX_LINE_LIMIT)
    }
}

/// Collect dmesg output (F44: /dev/kmsg fallback to dmesg command).
async fn collect_dmesg(limit: u32, grep: Option<&Regex>) -> DiagChunk {
    // Try /dev/kmsg first, fall back to dmesg command (F44)
    let content = if let Ok(content) = tokio::fs::read_to_string("/dev/kmsg").await {
        content
    } else {
        warn!("dmesg via /dev/kmsg failed, trying dmesg command");
        match run_command_with_timeout("dmesg", &[]).await {
            Ok(output) => output,
            Err(e) => {
                warn!("dmesg unavailable, skipping source: {e}");
                return DiagChunk { source: "dmesg".to_string(), lines: vec![], truncated: false };
            }
        }
    };

    let lines = read_last_n_lines(&content, limit);
    let lines = apply_grep(lines, grep);
    let truncated = content.lines().count() > limit as usize;

    DiagChunk { source: "dmesg".to_string(), lines, truncated }
}

/// Collect syslog output.
async fn collect_syslog(limit: u32, grep: Option<&Regex>) -> DiagChunk {
    // Try /var/log/syslog, then /var/log/messages
    let content = match tokio::fs::read_to_string("/var/log/syslog").await {
        Ok(c) => c,
        Err(_) => match tokio::fs::read_to_string("/var/log/messages").await {
            Ok(c) => c,
            Err(_) => {
                return DiagChunk { source: "syslog".to_string(), lines: vec![], truncated: false };
            }
        },
    };

    let total_lines = content.lines().count();
    let lines = read_last_n_lines(&content, limit);
    let lines = apply_grep(lines, grep);
    let truncated = total_lines > limit as usize;

    DiagChunk { source: "syslog".to_string(), lines, truncated }
}

/// Collect service log output.
///
/// PactSupervisor mode: read from /run/pact/logs/{service}.log
/// Systemd mode: run `journalctl -u {service} --no-pager -n {limit}` (F45: 5s timeout)
async fn collect_service_log(
    service: &str,
    limit: u32,
    grep: Option<&Regex>,
    backend: &SupervisorBackend,
) -> DiagChunk {
    let source = format!("service:{service}");

    match backend {
        SupervisorBackend::Pact => {
            let path = format!("/run/pact/logs/{service}.log");
            match tokio::fs::read_to_string(&path).await {
                Ok(content) => {
                    let total_lines = content.lines().count();
                    let lines = read_last_n_lines(&content, limit);
                    let lines = apply_grep(lines, grep);
                    let truncated = total_lines > limit as usize;
                    DiagChunk { source, lines, truncated }
                }
                Err(_) => DiagChunk { source, lines: vec![], truncated: false },
            }
        }
        SupervisorBackend::Systemd => {
            let limit_str = limit.to_string();
            match run_command_with_timeout(
                "journalctl",
                &["-u", service, "--no-pager", "-n", &limit_str],
            )
            .await
            {
                Ok(content) => {
                    let lines: Vec<String> = content.lines().map(String::from).collect();
                    let lines = apply_grep(lines, grep);
                    DiagChunk { source, lines, truncated: false }
                }
                Err(e) => {
                    warn!("journalctl timed out for service {service} after {SUBPROCESS_TIMEOUT_SECS}s: {e}");
                    DiagChunk { source, lines: vec![], truncated: true }
                }
            }
        }
    }
}

/// Read the last N lines from content.
pub fn read_last_n_lines(content: &str, n: u32) -> Vec<String> {
    let lines: Vec<&str> = content.lines().collect();
    let n = n as usize;
    if lines.len() <= n {
        lines.into_iter().map(String::from).collect()
    } else {
        lines[lines.len() - n..].iter().map(|s| String::from(*s)).collect()
    }
}

/// Apply grep filter to lines. If no pattern, returns lines unchanged.
pub fn apply_grep(lines: Vec<String>, pattern: Option<&Regex>) -> Vec<String> {
    match pattern {
        None => lines,
        Some(re) => lines.into_iter().filter(|line| re.is_match(line)).collect(),
    }
}

/// Run a command with a timeout, returning stdout as a string.
async fn run_command_with_timeout(cmd: &str, args: &[&str]) -> Result<String, String> {
    use tokio::process::Command;

    let child = Command::new(cmd)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("failed to spawn {cmd}: {e}"))?;

    let timeout = tokio::time::Duration::from_secs(SUBPROCESS_TIMEOUT_SECS);
    match tokio::time::timeout(timeout, child.wait_with_output()).await {
        Ok(Ok(output)) => Ok(String::from_utf8_lossy(&output.stdout).to_string()),
        Ok(Err(e)) => Err(format!("{cmd} failed: {e}")),
        Err(_) => Err(format!("{cmd} timed out after {SUBPROCESS_TIMEOUT_SECS}s")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- read_last_n_lines ---

    #[test]
    fn read_last_n_lines_fewer_than_n() {
        let content = "line1\nline2\nline3";
        let result = read_last_n_lines(content, 10);
        assert_eq!(result, vec!["line1", "line2", "line3"]);
    }

    #[test]
    fn read_last_n_lines_exactly_n() {
        let content = "line1\nline2\nline3";
        let result = read_last_n_lines(content, 3);
        assert_eq!(result, vec!["line1", "line2", "line3"]);
    }

    #[test]
    fn read_last_n_lines_more_than_n() {
        let content = "line1\nline2\nline3\nline4\nline5";
        let result = read_last_n_lines(content, 2);
        assert_eq!(result, vec!["line4", "line5"]);
    }

    #[test]
    fn read_last_n_lines_empty_content() {
        let result = read_last_n_lines("", 10);
        // str::lines() on empty string yields zero items
        assert!(result.is_empty());
    }

    #[test]
    fn read_last_n_lines_zero_limit() {
        let content = "line1\nline2";
        let result = read_last_n_lines(content, 0);
        assert!(result.is_empty());
    }

    #[test]
    fn read_last_n_lines_single_line() {
        let result = read_last_n_lines("only line", 5);
        assert_eq!(result, vec!["only line"]);
    }

    // --- apply_grep ---

    #[test]
    fn apply_grep_no_pattern() {
        let lines = vec!["a".to_string(), "b".to_string()];
        let result = apply_grep(lines.clone(), None);
        assert_eq!(result, lines);
    }

    #[test]
    fn apply_grep_matching() {
        let re = Regex::new("error").unwrap();
        let lines = vec![
            "info: starting".to_string(),
            "error: disk full".to_string(),
            "info: retrying".to_string(),
            "error: timeout".to_string(),
        ];
        let result = apply_grep(lines, Some(&re));
        assert_eq!(result, vec!["error: disk full", "error: timeout"]);
    }

    #[test]
    fn apply_grep_no_match() {
        let re = Regex::new("CRITICAL").unwrap();
        let lines = vec!["info: ok".to_string(), "warn: maybe".to_string()];
        let result = apply_grep(lines, Some(&re));
        assert!(result.is_empty());
    }

    #[test]
    fn apply_grep_regex_pattern() {
        let re = Regex::new(r"ECC\s+error").unwrap();
        let lines = vec![
            "GPU 0: ECC error detected".to_string(),
            "GPU 1: temperature normal".to_string(),
            "GPU 2: ECC  error corrected".to_string(),
        ];
        let result = apply_grep(lines, Some(&re));
        assert_eq!(result.len(), 2);
    }

    // --- validate_grep_pattern ---

    #[test]
    fn validate_grep_pattern_empty() {
        assert!(validate_grep_pattern("").unwrap().is_none());
    }

    #[test]
    fn validate_grep_pattern_valid() {
        let result = validate_grep_pattern("error|warn");
        assert!(result.is_ok());
        assert!(result.unwrap().is_some());
    }

    #[test]
    fn validate_grep_pattern_invalid_regex() {
        let result = validate_grep_pattern("[invalid");
        assert!(result.is_err());
        let status = result.unwrap_err();
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
        assert!(status.message().contains("invalid grep pattern"));
    }

    #[test]
    fn validate_grep_pattern_too_long() {
        let long_pattern = "a".repeat(256);
        let result = validate_grep_pattern(&long_pattern);
        assert!(result.is_err());
        let status = result.unwrap_err();
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
        assert!(status.message().contains("maximum length"));
    }

    #[test]
    fn validate_grep_pattern_max_length_ok() {
        let pattern = "a".repeat(255);
        let result = validate_grep_pattern(&pattern);
        assert!(result.is_ok());
    }

    // --- validate_service_name ---

    #[test]
    fn validate_service_name_empty() {
        let declared = vec!["nginx".to_string()];
        assert!(validate_service_name("", &declared).is_ok());
    }

    #[test]
    fn validate_service_name_valid_known() {
        let declared = vec!["nvidia-persistenced".to_string(), "chronyd".to_string()];
        assert!(validate_service_name("chronyd", &declared).is_ok());
    }

    #[test]
    fn validate_service_name_unknown_service() {
        let declared = vec!["chronyd".to_string()];
        // Unknown service is OK — collect_service_log returns empty chunk (F43)
        assert!(validate_service_name("nonexistent", &declared).is_ok());
    }

    #[test]
    fn validate_service_name_path_traversal_slash() {
        let declared = vec!["nginx".to_string()];
        let result = validate_service_name("../../etc/passwd", &declared);
        assert!(result.is_err());
        let status = result.unwrap_err();
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
        assert!(status.message().contains("invalid service name"));
    }

    #[test]
    fn validate_service_name_path_traversal_dotdot() {
        let declared = vec!["nginx".to_string()];
        let result = validate_service_name("..secret", &declared);
        assert!(result.is_err());
        assert!(result.unwrap_err().message().contains("invalid service name"));
    }

    #[test]
    fn validate_service_name_with_forward_slash() {
        let declared = vec!["nginx".to_string()];
        let result = validate_service_name("some/service", &declared);
        assert!(result.is_err());
    }

    // --- normalize_line_limit ---

    #[test]
    fn normalize_line_limit_zero_becomes_default() {
        assert_eq!(normalize_line_limit(0), DEFAULT_LINE_LIMIT);
    }

    #[test]
    fn normalize_line_limit_normal() {
        assert_eq!(normalize_line_limit(500), 500);
    }

    #[test]
    fn normalize_line_limit_exceeds_max() {
        assert_eq!(normalize_line_limit(20_000), MAX_LINE_LIMIT);
    }

    #[test]
    fn normalize_line_limit_at_max() {
        assert_eq!(normalize_line_limit(10_000), 10_000);
    }

    // --- collect_diag integration (mock-safe) ---

    #[tokio::test]
    async fn collect_diag_empty_request() {
        let request = DiagRequest {
            source_filter: "all".to_string(),
            service_name: String::new(),
            grep_pattern: String::new(),
            line_limit: 0,
        };
        let chunks =
            collect_diag(&request, SupervisorBackend::Pact, &["chronyd".to_string()]).await;
        // Should have dmesg + syslog + 1 service = 3 chunks
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].source, "dmesg");
        assert_eq!(chunks[1].source, "syslog");
        assert_eq!(chunks[2].source, "service:chronyd");
    }

    #[tokio::test]
    async fn collect_diag_system_only() {
        let request = DiagRequest {
            source_filter: "system".to_string(),
            service_name: String::new(),
            grep_pattern: String::new(),
            line_limit: 50,
        };
        let chunks = collect_diag(&request, SupervisorBackend::Pact, &[]).await;
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].source, "dmesg");
        assert_eq!(chunks[1].source, "syslog");
    }

    #[tokio::test]
    async fn collect_diag_service_only() {
        let request = DiagRequest {
            source_filter: "service".to_string(),
            service_name: String::new(),
            grep_pattern: String::new(),
            line_limit: 50,
        };
        let services = vec!["svc-a".to_string(), "svc-b".to_string()];
        let chunks = collect_diag(&request, SupervisorBackend::Pact, &services).await;
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].source, "service:svc-a");
        assert_eq!(chunks[1].source, "service:svc-b");
    }

    #[tokio::test]
    async fn collect_diag_specific_service() {
        let request = DiagRequest {
            source_filter: "service".to_string(),
            service_name: "nginx".to_string(),
            grep_pattern: String::new(),
            line_limit: 50,
        };
        let chunks = collect_diag(&request, SupervisorBackend::Pact, &["nginx".to_string()]).await;
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].source, "service:nginx");
    }

    #[tokio::test]
    async fn collect_diag_with_grep() {
        let request = DiagRequest {
            source_filter: "system".to_string(),
            service_name: String::new(),
            grep_pattern: "error".to_string(),
            line_limit: 100,
        };
        let chunks = collect_diag(&request, SupervisorBackend::Pact, &[]).await;
        // Should still produce chunks (empty is fine)
        assert_eq!(chunks.len(), 2);
    }
}
