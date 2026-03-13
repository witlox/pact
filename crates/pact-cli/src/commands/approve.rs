//! `pact approve` — two-person approval workflow.
//!
//! For regulated vClusters with `two_person_approval = true`,
//! state-changing operations require a second admin to approve.

use chrono::{DateTime, Utc};
use pact_common::types::{ApprovalStatus, PendingApproval, Scope};

/// Format a list of pending approvals for display.
pub fn format_approval_list(approvals: &[PendingApproval]) -> String {
    if approvals.is_empty() {
        return "(no pending approvals)".to_string();
    }

    let mut lines = vec![format!(
        "{:<10} {:<20} {:<30} {:<25} {}",
        "ID", "SCOPE", "ACTION", "REQUESTER", "AGE"
    )];

    for approval in approvals {
        let scope = format_scope(&approval.scope);
        let age = format_age(&approval.created_at);
        let status_marker = match approval.status {
            ApprovalStatus::Pending => "",
            ApprovalStatus::Approved => " [APPROVED]",
            ApprovalStatus::Rejected => " [REJECTED]",
            ApprovalStatus::Expired => " [EXPIRED]",
        };

        lines.push(format!(
            "{:<10} {:<20} {:<30} {:<25} {}{}",
            truncate(&approval.approval_id, 10),
            truncate(&scope, 20),
            truncate(&approval.action, 30),
            truncate(&approval.requester.principal, 25),
            age,
            status_marker,
        ));
    }

    lines.join("\n")
}

/// Format approval result for display.
pub fn format_approve_result(approval_id: &str, action: &str) -> String {
    match action {
        "approve" => format!("Approved {}. Original operation will proceed.", approval_id),
        "deny" => format!("Denied {}. Original operation cancelled.", approval_id),
        _ => format!("{}: {}", action, approval_id),
    }
}

/// Validate that an approval is still actionable.
pub fn validate_approval(approval: &PendingApproval, approver: &str) -> Result<(), String> {
    if approval.status != ApprovalStatus::Pending {
        return Err(format!("Approval {} is already {:?}", approval.approval_id, approval.status));
    }

    if approval.requester.principal == approver {
        return Err(format!("Cannot approve your own request (P4: self-approval denied)"));
    }

    if Utc::now() > approval.expires_at {
        return Err(format!("Approval {} has expired (P5)", approval.approval_id));
    }

    Ok(())
}

fn format_scope(scope: &Scope) -> String {
    match scope {
        Scope::Global => "global".to_string(),
        Scope::VCluster(vc) => vc.clone(),
        Scope::Node(n) => n.clone(),
    }
}

fn format_age(created: &DateTime<Utc>) -> String {
    let elapsed = (Utc::now() - *created).num_seconds();
    if elapsed < 60 {
        format!("{}s ago", elapsed)
    } else if elapsed < 3600 {
        format!("{} min ago", elapsed / 60)
    } else {
        format!("{} hr ago", elapsed / 3600)
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}…", &s[..max_len - 1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pact_common::types::{Identity, PrincipalType};

    fn test_approval(status: ApprovalStatus, expires_offset_secs: i64) -> PendingApproval {
        PendingApproval {
            approval_id: "ap-7f3a".into(),
            original_request: "commit config".into(),
            action: "commit".into(),
            scope: Scope::VCluster("sensitive-compute".into()),
            requester: Identity {
                principal: "admin-a@org".into(),
                principal_type: PrincipalType::Human,
                role: "pact-regulated-sensitive-compute".into(),
            },
            approver: None,
            status,
            created_at: Utc::now() - chrono::Duration::seconds(720),
            expires_at: Utc::now() + chrono::Duration::seconds(expires_offset_secs),
        }
    }

    #[test]
    fn format_approval_list_empty() {
        assert_eq!(format_approval_list(&[]), "(no pending approvals)");
    }

    #[test]
    fn format_approval_list_with_entries() {
        let approvals = vec![test_approval(ApprovalStatus::Pending, 1800)];
        let output = format_approval_list(&approvals);
        assert!(output.contains("ap-7f3a"));
        assert!(output.contains("sensitive-compute"));
        assert!(output.contains("admin-a@org"));
        assert!(output.contains("min ago"));
    }

    #[test]
    fn format_approval_list_shows_status() {
        let approvals = vec![
            test_approval(ApprovalStatus::Approved, 1800),
            test_approval(ApprovalStatus::Rejected, 1800),
        ];
        let output = format_approval_list(&approvals);
        assert!(output.contains("[APPROVED]"));
        assert!(output.contains("[REJECTED]"));
    }

    #[test]
    fn format_approve_result_approve() {
        let output = format_approve_result("ap-7f3a", "approve");
        assert!(output.contains("Approved"));
        assert!(output.contains("proceed"));
    }

    #[test]
    fn format_approve_result_deny() {
        let output = format_approve_result("ap-7f3a", "deny");
        assert!(output.contains("Denied"));
        assert!(output.contains("cancelled"));
    }

    #[test]
    fn validate_pending_approval_ok() {
        let approval = test_approval(ApprovalStatus::Pending, 1800);
        let result = validate_approval(&approval, "admin-b@org");
        assert!(result.is_ok());
    }

    #[test]
    fn validate_already_approved_fails() {
        let approval = test_approval(ApprovalStatus::Approved, 1800);
        let result = validate_approval(&approval, "admin-b@org");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("already"));
    }

    #[test]
    fn validate_self_approval_fails() {
        let approval = test_approval(ApprovalStatus::Pending, 1800);
        let result = validate_approval(&approval, "admin-a@org");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("P4"));
    }

    #[test]
    fn validate_expired_approval_fails() {
        let approval = test_approval(ApprovalStatus::Pending, -60);
        let result = validate_approval(&approval, "admin-b@org");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("P5"));
    }

    #[test]
    fn truncate_short_string() {
        assert_eq!(truncate("abc", 10), "abc");
    }

    #[test]
    fn truncate_long_string() {
        assert_eq!(truncate("abcdefghijk", 5), "abcd…");
    }

    #[test]
    fn format_age_seconds() {
        let ts = Utc::now() - chrono::Duration::seconds(30);
        let output = format_age(&ts);
        assert!(output.contains("s ago"));
    }

    #[test]
    fn format_age_minutes() {
        let ts = Utc::now() - chrono::Duration::seconds(300);
        let output = format_age(&ts);
        assert!(output.contains("min ago"));
    }

    #[test]
    fn format_age_hours() {
        let ts = Utc::now() - chrono::Duration::seconds(7200);
        let output = format_age(&ts);
        assert!(output.contains("hr ago"));
    }
}
