//! `pact group` — vCluster group management.
//!
//! Query and manage vCluster configurations, member nodes, and policies.

use pact_common::types::VClusterPolicy;

/// Summary of a vCluster for list display.
#[derive(Debug, Clone)]
pub struct GroupSummary {
    pub name: String,
    pub node_count: u32,
    pub enforcement_mode: String,
    pub two_person_approval: bool,
}

/// Detailed info for a vCluster.
#[derive(Debug, Clone)]
pub struct GroupDetail {
    pub name: String,
    pub policy: VClusterPolicy,
    pub node_ids: Vec<String>,
}

/// Format group list for display.
pub fn format_group_list(groups: &[GroupSummary]) -> String {
    if groups.is_empty() {
        return "No vClusters configured.".into();
    }

    let mut output = format!("{:<20} {:<8} {:<12} {}\n", "VCLUSTER", "NODES", "MODE", "2PA");
    output.push_str(&"-".repeat(52));
    output.push('\n');

    for g in groups {
        output.push_str(&format!(
            "{:<20} {:<8} {:<12} {}\n",
            g.name,
            g.node_count,
            g.enforcement_mode,
            if g.two_person_approval { "yes" } else { "no" },
        ));
    }
    output
}

/// Format group detail for display.
pub fn format_group_detail(detail: &GroupDetail) -> String {
    let mut output = format!("vCluster: {}\n", detail.name);
    output.push_str(&format!("  enforcement: {}\n", detail.policy.enforcement_mode,));
    output.push_str(&format!("  two-person approval: {}\n", detail.policy.two_person_approval,));
    output.push_str(&format!(
        "  base commit window: {}s\n",
        detail.policy.base_commit_window_seconds,
    ));
    output.push_str(&format!("  drift sensitivity: {:.1}\n", detail.policy.drift_sensitivity,));
    output.push_str(&format!("  nodes: {}\n", detail.node_ids.len()));
    for node in &detail.node_ids {
        output.push_str(&format!("    - {node}\n"));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_empty_group_list() {
        let output = format_group_list(&[]);
        assert!(output.contains("No vClusters"));
    }

    #[test]
    fn format_group_list_with_entries() {
        let groups = vec![
            GroupSummary {
                name: "ml-training".into(),
                node_count: 128,
                enforcement_mode: "enforce".into(),
                two_person_approval: false,
            },
            GroupSummary {
                name: "regulated".into(),
                node_count: 32,
                enforcement_mode: "enforce".into(),
                two_person_approval: true,
            },
        ];
        let output = format_group_list(&groups);
        assert!(output.contains("ml-training"));
        assert!(output.contains("128"));
        assert!(output.contains("regulated"));
        assert!(output.contains("yes"));
    }

    #[test]
    fn format_group_detail_shows_policy() {
        let detail = GroupDetail {
            name: "ml-training".into(),
            policy: VClusterPolicy::default(),
            node_ids: vec!["node-001".into(), "node-002".into()],
        };
        let output = format_group_detail(&detail);
        assert!(output.contains("ml-training"));
        assert!(output.contains("observe")); // default enforcement mode
        assert!(output.contains("node-001"));
        assert!(output.contains("nodes: 2"));
    }
}
