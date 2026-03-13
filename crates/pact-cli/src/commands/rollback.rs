//! `pact rollback` — roll back to a previous configuration state.

use pact_common::types::Scope;

/// Result of a rollback operation.
#[derive(Debug, Clone)]
pub struct RollbackResult {
    pub rollback_sequence: u64,
    pub target_sequence: u64,
    pub scope: Scope,
    pub entries_reverted: u32,
}

/// Format rollback result for display.
pub fn format_rollback_result(result: &RollbackResult) -> String {
    format!(
        "Rolled back to seq:{} ({} entries reverted). New entry: seq:{} on {}",
        result.target_sequence,
        result.entries_reverted,
        result.rollback_sequence,
        format_scope(&result.scope),
    )
}

/// Validate rollback target.
pub fn validate_rollback_target(target_seq: u64, current_seq: u64) -> Result<(), String> {
    if target_seq >= current_seq {
        return Err(format!(
            "Cannot rollback to seq:{} — current is seq:{}. Target must be earlier.",
            target_seq, current_seq,
        ));
    }
    if target_seq == 0 {
        return Err("Cannot rollback to seq:0 (no entries)".into());
    }
    Ok(())
}

fn format_scope(scope: &Scope) -> String {
    match scope {
        Scope::Global => "global".to_string(),
        Scope::VCluster(vc) => format!("vcluster: {}", vc),
        Scope::Node(n) => format!("node: {}", n),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_rollback_success() {
        let result = RollbackResult {
            rollback_sequence: 4815,
            target_sequence: 4810,
            scope: Scope::Node("node042".into()),
            entries_reverted: 5,
        };
        let output = format_rollback_result(&result);
        assert!(output.contains("seq:4810"));
        assert!(output.contains("5 entries reverted"));
        assert!(output.contains("seq:4815"));
    }

    #[test]
    fn validate_target_future_fails() {
        let result = validate_rollback_target(100, 50);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("must be earlier"));
    }

    #[test]
    fn validate_target_same_fails() {
        let result = validate_rollback_target(50, 50);
        assert!(result.is_err());
    }

    #[test]
    fn validate_target_zero_fails() {
        let result = validate_rollback_target(0, 50);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("seq:0"));
    }

    #[test]
    fn validate_target_valid() {
        let result = validate_rollback_target(45, 50);
        assert!(result.is_ok());
    }
}
