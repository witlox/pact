//! `pact log` — configuration history from the journal.

use chrono::{DateTime, Utc};
use pact_common::types::{ConfigEntry, EntryType, Scope};

/// Format a log entry for text display.
pub fn format_log_entry(entry: &ConfigEntry) -> String {
    let scope = format_scope(&entry.scope);
    let entry_type = format_entry_type(&entry.entry_type);
    let author = &entry.author.principal;

    let mut line = format!(
        "#{:<6} {} {:16} {:12} by {}",
        entry.sequence,
        format_timestamp(&entry.timestamp),
        entry_type,
        scope,
        author,
    );

    if let Some(ref reason) = entry.emergency_reason {
        line.push_str(&format!("  reason: {reason}"));
    }

    if let Some(ref policy_ref) = entry.policy_ref {
        line.push_str(&format!("  policy: {policy_ref}"));
    }

    line
}

/// Format a list of log entries for text display.
pub fn format_log(entries: &[ConfigEntry]) -> String {
    if entries.is_empty() {
        return "(no log entries)".to_string();
    }

    entries.iter().map(format_log_entry).collect::<Vec<_>>().join("\n")
}

fn format_scope(scope: &Scope) -> String {
    match scope {
        Scope::Global => "global".to_string(),
        Scope::VCluster(vc) => format!("vc:{vc}"),
        Scope::Node(n) => format!("node:{n}"),
    }
}

fn format_entry_type(et: &EntryType) -> &'static str {
    match et {
        EntryType::Commit => "COMMIT",
        EntryType::Rollback => "ROLLBACK",
        EntryType::AutoConverge => "AUTO_CONVERGE",
        EntryType::DriftDetected => "DRIFT_DETECTED",
        EntryType::CapabilityChange => "CAP_CHANGE",
        EntryType::PolicyUpdate => "POLICY_UPDATE",
        EntryType::BootConfig => "BOOT_CONFIG",
        EntryType::EmergencyStart => "EMERGENCY_ON",
        EntryType::EmergencyEnd => "EMERGENCY_OFF",
        EntryType::ExecLog => "EXEC",
        EntryType::ShellSession => "SHELL",
        EntryType::ServiceLifecycle => "SERVICE",
        EntryType::PendingApproval => "APPROVAL",
    }
}

fn format_timestamp(ts: &DateTime<Utc>) -> String {
    ts.format("%Y-%m-%d %H:%M:%S").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pact_common::types::{Identity, PrincipalType};

    fn test_entry(seq: u64, entry_type: EntryType, scope: Scope) -> ConfigEntry {
        ConfigEntry {
            sequence: seq,
            timestamp: Utc::now(),
            entry_type,
            scope,
            author: Identity {
                principal: "admin@example.com".into(),
                principal_type: PrincipalType::Human,
                role: "pact-platform-admin".into(),
            },
            parent: None,
            state_delta: None,
            policy_ref: None,
            ttl_seconds: None,
            emergency_reason: None,
        }
    }

    #[test]
    fn format_log_entry_commit() {
        let entry = test_entry(42, EntryType::Commit, Scope::Node("node042".into()));
        let output = format_log_entry(&entry);
        assert!(output.contains("#42"));
        assert!(output.contains("COMMIT"));
        assert!(output.contains("node:node042"));
        assert!(output.contains("admin@example.com"));
    }

    #[test]
    fn format_log_entry_emergency_with_reason() {
        let mut entry = test_entry(100, EntryType::EmergencyStart, Scope::Global);
        entry.emergency_reason = Some("GPU failure on node042".into());
        let output = format_log_entry(&entry);
        assert!(output.contains("EMERGENCY_ON"));
        assert!(output.contains("GPU failure"));
    }

    #[test]
    fn format_log_entry_with_policy_ref() {
        let mut entry = test_entry(50, EntryType::PolicyUpdate, Scope::VCluster("ml-train".into()));
        entry.policy_ref = Some("pol-abc123".into());
        let output = format_log_entry(&entry);
        assert!(output.contains("POLICY_UPDATE"));
        assert!(output.contains("vc:ml-train"));
        assert!(output.contains("pol-abc123"));
    }

    #[test]
    fn format_log_empty() {
        assert_eq!(format_log(&[]), "(no log entries)");
    }

    #[test]
    fn format_log_multiple_entries() {
        let entries = vec![
            test_entry(1, EntryType::BootConfig, Scope::Global),
            test_entry(2, EntryType::Commit, Scope::Node("node001".into())),
            test_entry(3, EntryType::DriftDetected, Scope::Node("node002".into())),
        ];
        let output = format_log(&entries);
        assert!(output.contains("#1"));
        assert!(output.contains("#2"));
        assert!(output.contains("#3"));
        assert!(output.contains("BOOT_CONFIG"));
        assert!(output.contains("COMMIT"));
        assert!(output.contains("DRIFT_DETECTED"));
    }

    #[test]
    fn format_scope_values() {
        assert_eq!(format_scope(&Scope::Global), "global");
        assert_eq!(format_scope(&Scope::VCluster("ml".into())), "vc:ml");
        assert_eq!(format_scope(&Scope::Node("n01".into())), "node:n01");
    }

    #[test]
    fn format_entry_type_all_variants() {
        assert_eq!(format_entry_type(&EntryType::Commit), "COMMIT");
        assert_eq!(format_entry_type(&EntryType::Rollback), "ROLLBACK");
        assert_eq!(format_entry_type(&EntryType::ExecLog), "EXEC");
        assert_eq!(format_entry_type(&EntryType::ShellSession), "SHELL");
        assert_eq!(format_entry_type(&EntryType::PendingApproval), "APPROVAL");
    }
}
