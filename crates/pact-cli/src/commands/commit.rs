//! `pact commit` — commit drift as a configuration entry.

use pact_common::types::{EntryType, Scope};

/// Result of a commit operation.
#[derive(Debug, Clone)]
pub struct CommitResult {
    pub sequence: u64,
    pub scope: Scope,
    pub policy_ref: Option<String>,
    pub approval_required: bool,
    pub approval_id: Option<String>,
}

/// Format commit result for display.
pub fn format_commit_result(result: &CommitResult) -> String {
    if result.approval_required {
        let id = result.approval_id.as_deref().unwrap_or("unknown");
        format!(
            "Approval required (two-person policy on {})\nPending approval: {} (expires in 30 min)\nWaiting for approval... (Ctrl-C to background)",
            format_scope(&result.scope),
            id,
        )
    } else {
        let mut line =
            format!("Committed (seq:{}) on {}", result.sequence, format_scope(&result.scope),);
        if let Some(ref policy_ref) = result.policy_ref {
            line.push_str(&format!("  policy: {policy_ref}"));
        }
        line
    }
}

/// Validate commit arguments.
pub fn validate_commit_args(message: Option<&str>, entry_type: &EntryType) -> Result<(), String> {
    if *entry_type == EntryType::Commit && message.is_none() {
        return Err("Commit message required (-m \"message\")".into());
    }
    Ok(())
}

fn format_scope(scope: &Scope) -> String {
    match scope {
        Scope::Global => "global".to_string(),
        Scope::VCluster(vc) => format!("vcluster: {vc}"),
        Scope::Node(n) => format!("node: {n}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_commit_success() {
        let result = CommitResult {
            sequence: 4812,
            scope: Scope::Node("node042".into()),
            policy_ref: Some("pol-123".into()),
            approval_required: false,
            approval_id: None,
        };
        let output = format_commit_result(&result);
        assert!(output.contains("seq:4812"));
        assert!(output.contains("node: node042"));
        assert!(output.contains("pol-123"));
    }

    #[test]
    fn format_commit_approval_required() {
        let result = CommitResult {
            sequence: 0,
            scope: Scope::VCluster("sensitive-compute".into()),
            policy_ref: None,
            approval_required: true,
            approval_id: Some("ap-7f3a".into()),
        };
        let output = format_commit_result(&result);
        assert!(output.contains("Approval required"));
        assert!(output.contains("sensitive-compute"));
        assert!(output.contains("ap-7f3a"));
    }

    #[test]
    fn validate_commit_requires_message() {
        let result = validate_commit_args(None, &EntryType::Commit);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("message required"));
    }

    #[test]
    fn validate_commit_with_message_ok() {
        let result = validate_commit_args(Some("add hugepages"), &EntryType::Commit);
        assert!(result.is_ok());
    }

    #[test]
    fn validate_rollback_no_message_ok() {
        // Rollbacks don't require a message
        let result = validate_commit_args(None, &EntryType::Rollback);
        assert!(result.is_ok());
    }
}
