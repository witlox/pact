//! `pact emergency` — enter/exit emergency mode.
//!
//! Emergency mode (ADR-004):
//! - Extends commit window to emergency_window_seconds (default 4h)
//! - Preserves audit trail — every action still logged
//! - Cannot be entered by AI agents (P8)
//! - Different admin can force-end another admin's emergency

use pact_common::types::PrincipalType;

/// Emergency mode subcommand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmergencyAction {
    Start { reason: String },
    End { force: bool },
}

/// Result of an emergency mode operation.
#[derive(Debug, Clone)]
pub struct EmergencyResult {
    pub action: String,
    pub node_id: String,
    pub sequence: u64,
    pub window_seconds: Option<u32>,
}

/// Format emergency result for display.
pub fn format_emergency_result(result: &EmergencyResult) -> String {
    match result.action.as_str() {
        "start" => {
            let window_min = result.window_seconds.unwrap_or(14400) / 60;
            format!(
                "Emergency mode ACTIVE on {} (seq:{})\nExtended commit window: {} minutes\nAll actions continue to be logged.",
                result.node_id, result.sequence, window_min,
            )
        }
        "end" => {
            format!(
                "Emergency mode ENDED on {} (seq:{})\nNormal commit window restored.",
                result.node_id, result.sequence,
            )
        }
        _ => format!("Emergency: {} on {}", result.action, result.node_id),
    }
}

/// Validate emergency start constraints (P8: AI agents cannot start emergency).
pub fn validate_emergency_start(
    principal_type: &PrincipalType,
    reason: &str,
) -> Result<(), String> {
    if *principal_type == PrincipalType::Agent || *principal_type == PrincipalType::Service {
        return Err("AI/service agents cannot enter emergency mode (P8)".into());
    }
    if reason.trim().is_empty() {
        return Err("Emergency reason is required".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_emergency_start() {
        let result = EmergencyResult {
            action: "start".into(),
            node_id: "node042".into(),
            sequence: 5001,
            window_seconds: Some(14400),
        };
        let output = format_emergency_result(&result);
        assert!(output.contains("ACTIVE"));
        assert!(output.contains("node042"));
        assert!(output.contains("240 minutes"));
        assert!(output.contains("logged"));
    }

    #[test]
    fn format_emergency_end() {
        let result = EmergencyResult {
            action: "end".into(),
            node_id: "node042".into(),
            sequence: 5010,
            window_seconds: None,
        };
        let output = format_emergency_result(&result);
        assert!(output.contains("ENDED"));
        assert!(output.contains("restored"));
    }

    #[test]
    fn validate_ai_agent_blocked() {
        let result = validate_emergency_start(&PrincipalType::Agent, "gpu failure");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("P8"));
    }

    #[test]
    fn validate_service_blocked() {
        let result = validate_emergency_start(&PrincipalType::Service, "gpu failure");
        assert!(result.is_err());
    }

    #[test]
    fn validate_human_allowed() {
        let result = validate_emergency_start(&PrincipalType::Human, "gpu failure on rack3");
        assert!(result.is_ok());
    }

    #[test]
    fn validate_empty_reason_rejected() {
        let result = validate_emergency_start(&PrincipalType::Human, "  ");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("reason"));
    }

    #[test]
    fn emergency_action_equality() {
        let a = EmergencyAction::Start { reason: "test".into() };
        let b = EmergencyAction::Start { reason: "test".into() };
        assert_eq!(a, b);
        assert_ne!(a, EmergencyAction::End { force: false });
    }
}
