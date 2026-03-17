//! `pact promote` — export committed node deltas as overlay TOML.
//!
//! Workflow: node-level experiments → commit → promote to vCluster overlay.
//! Maps StateDelta fields to overlay TOML sections.

use pact_common::types::{DeltaAction, DeltaItem, StateDelta};

/// Result of a promote operation.
#[derive(Debug, Clone)]
pub struct PromoteResult {
    pub node_id: String,
    pub vcluster_id: String,
    pub deltas_exported: u32,
    pub overlay_toml: String,
    pub conflicts: Vec<PromoteConflict>,
    pub dry_run: bool,
}

/// Conflict between promoted delta and existing node state.
#[derive(Debug, Clone)]
pub struct PromoteConflict {
    pub key: String,
    pub promoted_value: String,
    pub existing_value: String,
    pub node_id: String,
}

/// Export committed node deltas as overlay TOML.
pub fn export_deltas_as_toml(deltas: &[StateDelta]) -> String {
    let mut toml = String::new();
    toml.push_str("# Promoted from node deltas\n");
    toml.push_str("# Review and apply with: pact apply <this-file.toml>\n\n");

    for delta in deltas {
        write_section(&mut toml, "kernel", &delta.kernel);
        write_section(&mut toml, "services", &delta.services);
        write_section(&mut toml, "mounts", &delta.mounts);
        write_section(&mut toml, "files", &delta.files);
        write_section(&mut toml, "network", &delta.network);
        write_section(&mut toml, "packages", &delta.packages);
        write_section(&mut toml, "gpu", &delta.gpu);
    }

    toml
}

fn write_section(toml: &mut String, name: &str, items: &[DeltaItem]) {
    let active: Vec<_> = items.iter().filter(|i| i.action != DeltaAction::Remove).collect();
    if active.is_empty() {
        return;
    }
    toml.push_str(&format!("[{name}]\n"));
    for item in active {
        if let Some(ref value) = item.value {
            toml.push_str(&format!("{} = \"{value}\"\n", item.key));
        } else {
            toml.push_str(&format!("# {} (action: {:?})\n", item.key, item.action));
        }
    }
    toml.push('\n');
}

/// Format promote result for display.
pub fn format_promote_result(result: &PromoteResult) -> String {
    let mut output = String::new();

    if result.dry_run {
        output.push_str("DRY RUN — no changes applied\n\n");
    }

    output.push_str(&format!(
        "Promoted {} delta(s) from node {} to vCluster {}\n",
        result.deltas_exported, result.node_id, result.vcluster_id,
    ));

    if !result.conflicts.is_empty() {
        output.push_str(&format!("\n{} conflict(s) detected:\n", result.conflicts.len()));
        for c in &result.conflicts {
            output.push_str(&format!(
                "  {} on {}: promoted=\"{}\" vs existing=\"{}\"\n",
                c.key, c.node_id, c.promoted_value, c.existing_value,
            ));
        }
    }

    if !result.dry_run && result.conflicts.is_empty() {
        output.push_str("\nOverlay updated. Subscribed agents will receive the update.\n");
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_delta() -> StateDelta {
        StateDelta {
            kernel: vec![
                DeltaItem {
                    action: DeltaAction::Modify,
                    key: "net.core.somaxconn".into(),
                    value: Some("2048".into()),
                    previous: Some("128".into()),
                },
                DeltaItem {
                    action: DeltaAction::Modify,
                    key: "vm.swappiness".into(),
                    value: Some("10".into()),
                    previous: Some("60".into()),
                },
            ],
            ..StateDelta::default()
        }
    }

    #[test]
    fn export_deltas_produces_toml() {
        let deltas = vec![test_delta()];
        let toml = export_deltas_as_toml(&deltas);
        assert!(toml.contains("[kernel]"));
        assert!(toml.contains("net.core.somaxconn"));
        assert!(toml.contains("vm.swappiness"));
    }

    #[test]
    fn export_empty_deltas() {
        let toml = export_deltas_as_toml(&[]);
        assert!(toml.contains("Promoted from node deltas"));
        assert!(!toml.contains("[kernel]"));
    }

    #[test]
    fn export_skips_removed_items() {
        let delta = StateDelta {
            kernel: vec![DeltaItem {
                action: DeltaAction::Remove,
                key: "old.param".into(),
                value: None,
                previous: Some("old".into()),
            }],
            ..StateDelta::default()
        };
        let toml = export_deltas_as_toml(&[delta]);
        assert!(!toml.contains("old.param"));
    }

    #[test]
    fn format_promote_dry_run() {
        let result = PromoteResult {
            node_id: "node-001".into(),
            vcluster_id: "ml-training".into(),
            deltas_exported: 3,
            overlay_toml: String::new(),
            conflicts: vec![],
            dry_run: true,
        };
        let output = format_promote_result(&result);
        assert!(output.contains("DRY RUN"));
        assert!(output.contains("3 delta(s)"));
    }

    #[test]
    fn format_promote_with_conflicts() {
        let result = PromoteResult {
            node_id: "node-001".into(),
            vcluster_id: "ml-training".into(),
            deltas_exported: 1,
            overlay_toml: String::new(),
            conflicts: vec![PromoteConflict {
                key: "vm.swappiness".into(),
                promoted_value: "10".into(),
                existing_value: "60".into(),
                node_id: "node-002".into(),
            }],
            dry_run: false,
        };
        let output = format_promote_result(&result);
        assert!(output.contains("1 conflict(s)"));
        assert!(output.contains("vm.swappiness"));
    }
}
