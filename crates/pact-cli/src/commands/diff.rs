//! `pact diff` — show declared vs actual state differences.

use pact_common::types::{DeltaAction, DeltaItem, StateDelta};

/// Format a state delta for display.
pub fn format_diff(delta: &StateDelta) -> String {
    let mut lines = Vec::new();

    format_delta_section(&mut lines, "kernel", &delta.kernel);
    format_delta_section(&mut lines, "mounts", &delta.mounts);
    format_delta_section(&mut lines, "files", &delta.files);
    format_delta_section(&mut lines, "network", &delta.network);
    format_delta_section(&mut lines, "services", &delta.services);
    format_delta_section(&mut lines, "packages", &delta.packages);
    format_delta_section(&mut lines, "gpu", &delta.gpu);

    if lines.is_empty() {
        return "(no differences)".to_string();
    }

    lines.join("\n")
}

fn format_delta_section(lines: &mut Vec<String>, category: &str, items: &[DeltaItem]) {
    for item in items {
        let prefix = match item.action {
            DeltaAction::Add => "+",
            DeltaAction::Remove => "-",
            DeltaAction::Modify => "~",
        };

        let detail = match (&item.value, &item.previous) {
            (Some(val), Some(prev)) => format!("{}: {} → {}", item.key, prev, val),
            (Some(val), None) => format!("{}: {}", item.key, val),
            (None, Some(prev)) => format!("{}: {} (removed)", item.key, prev),
            (None, None) => item.key.clone(),
        };

        lines.push(format!("  {} {}: {}", prefix, category, detail));
    }
}

/// Format a committed diff (node deltas not yet promoted).
pub fn format_committed_diff(node_id: &str, deltas: &[(u64, String, StateDelta)]) -> String {
    if deltas.is_empty() {
        return format!("(no committed node deltas on {})", node_id);
    }

    let mut lines = vec![format!("Committed node deltas on {} ({} total):", node_id, deltas.len())];

    for (seq, timestamp, delta) in deltas {
        lines.push(format!("  seq:{} ({})", seq, timestamp));
        let diff = format_diff(delta);
        for line in diff.lines() {
            lines.push(format!("    {}", line));
        }
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_diff_no_changes() {
        let delta = StateDelta::default();
        assert_eq!(format_diff(&delta), "(no differences)");
    }

    #[test]
    fn format_diff_kernel_modify() {
        let delta = StateDelta {
            kernel: vec![DeltaItem {
                action: DeltaAction::Modify,
                key: "vm.nr_hugepages".into(),
                value: Some("1024".into()),
                previous: Some("0".into()),
            }],
            ..Default::default()
        };
        let output = format_diff(&delta);
        assert!(output.contains("~ kernel: vm.nr_hugepages: 0 → 1024"));
    }

    #[test]
    fn format_diff_mount_add() {
        let delta = StateDelta {
            mounts: vec![DeltaItem {
                action: DeltaAction::Add,
                key: "/scratch".into(),
                value: Some("nfs:storage03:/scratch".into()),
                previous: None,
            }],
            ..Default::default()
        };
        let output = format_diff(&delta);
        assert!(output.contains("+ mounts: /scratch: nfs:storage03:/scratch"));
    }

    #[test]
    fn format_diff_service_remove() {
        let delta = StateDelta {
            services: vec![DeltaItem {
                action: DeltaAction::Remove,
                key: "old-agent".into(),
                value: None,
                previous: Some("running".into()),
            }],
            ..Default::default()
        };
        let output = format_diff(&delta);
        assert!(output.contains("- services: old-agent: running (removed)"));
    }

    #[test]
    fn format_diff_multiple_categories() {
        let delta = StateDelta {
            kernel: vec![DeltaItem {
                action: DeltaAction::Modify,
                key: "vm.swappiness".into(),
                value: Some("10".into()),
                previous: Some("60".into()),
            }],
            files: vec![DeltaItem {
                action: DeltaAction::Add,
                key: "/etc/pact/local.conf".into(),
                value: Some("hash:abc123".into()),
                previous: None,
            }],
            ..Default::default()
        };
        let output = format_diff(&delta);
        assert!(output.contains("kernel"));
        assert!(output.contains("files"));
    }

    #[test]
    fn format_committed_diff_empty() {
        let output = format_committed_diff("node042", &[]);
        assert!(output.contains("no committed node deltas"));
    }

    #[test]
    fn format_committed_diff_with_entries() {
        let deltas = vec![(
            4812,
            "2026-03-10T14:00:00Z".to_string(),
            StateDelta {
                kernel: vec![DeltaItem {
                    action: DeltaAction::Modify,
                    key: "vm.nr_hugepages".into(),
                    value: Some("1024".into()),
                    previous: Some("0".into()),
                }],
                ..Default::default()
            },
        )];
        let output = format_committed_diff("node042", &deltas);
        assert!(output.contains("seq:4812"));
        assert!(output.contains("vm.nr_hugepages"));
    }
}
