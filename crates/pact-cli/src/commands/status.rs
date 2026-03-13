//! `pact status` — show node/vCluster state, drift, capabilities.

use pact_common::types::{ConfigState, DriftVector, GpuHealth, SupervisorStatus};

/// Formatted status output for a node.
#[derive(Debug, Clone)]
pub struct NodeStatus {
    pub node_id: String,
    pub vcluster_id: String,
    pub config_state: ConfigState,
    pub drift_summary: Option<DriftVector>,
    pub supervisor: SupervisorStatus,
    pub gpu_count: u32,
    pub gpu_healthy: u32,
    pub gpu_degraded: u32,
    pub memory_total_gb: f64,
    pub memory_avail_gb: f64,
}

/// Format a single node status for text output.
pub fn format_node_status(status: &NodeStatus) -> String {
    let mut lines = Vec::new();

    lines.push(format!(
        "Node: {}  vCluster: {}  State: {}",
        status.node_id,
        status.vcluster_id,
        format_config_state(&status.config_state),
    ));

    lines.push(format!(
        "Supervisor: {} ({}/{} running, {} failed)",
        format_supervisor_backend(&status.supervisor),
        status.supervisor.services_running,
        status.supervisor.services_declared,
        status.supervisor.services_failed,
    ));

    lines.push(format!(
        "Memory: {:.1} GB / {:.1} GB available",
        status.memory_avail_gb, status.memory_total_gb,
    ));

    if status.gpu_count > 0 {
        lines.push(format!(
            "GPUs: {} total ({} healthy, {} degraded)",
            status.gpu_count, status.gpu_healthy, status.gpu_degraded,
        ));
    }

    if let Some(ref drift) = status.drift_summary {
        let categories = format_drift_categories(drift);
        if !categories.is_empty() {
            lines.push(format!("Drift: {categories}"));
        }
    }

    lines.join("\n")
}

fn format_config_state(state: &ConfigState) -> &'static str {
    match state {
        ConfigState::Committed => "COMMITTED",
        ConfigState::Drifted => "DRIFTED",
        ConfigState::Converging => "CONVERGING",
        ConfigState::Emergency => "EMERGENCY",
        ConfigState::ObserveOnly => "OBSERVE_ONLY",
    }
}

fn format_supervisor_backend(status: &SupervisorStatus) -> &'static str {
    match status.backend {
        pact_common::types::SupervisorBackend::Pact => "pact",
        pact_common::types::SupervisorBackend::Systemd => "systemd",
    }
}

fn format_drift_categories(drift: &DriftVector) -> String {
    let mut parts = Vec::new();
    if drift.kernel > 0.0 {
        parts.push(format!("kernel({:.1})", drift.kernel));
    }
    if drift.mounts > 0.0 {
        parts.push(format!("mounts({:.1})", drift.mounts));
    }
    if drift.files > 0.0 {
        parts.push(format!("files({:.1})", drift.files));
    }
    if drift.network > 0.0 {
        parts.push(format!("network({:.1})", drift.network));
    }
    if drift.services > 0.0 {
        parts.push(format!("services({:.1})", drift.services));
    }
    if drift.packages > 0.0 {
        parts.push(format!("packages({:.1})", drift.packages));
    }
    if drift.gpu > 0.0 {
        parts.push(format!("gpu({:.1})", drift.gpu));
    }
    parts.join(", ")
}

/// Format GPU health for status display.
pub fn format_gpu_health(health: &GpuHealth) -> &'static str {
    match health {
        GpuHealth::Healthy => "healthy",
        GpuHealth::Degraded => "DEGRADED",
        GpuHealth::Failed => "FAILED",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pact_common::types::SupervisorBackend;

    fn test_status() -> NodeStatus {
        NodeStatus {
            node_id: "node042".into(),
            vcluster_id: "ml-training".into(),
            config_state: ConfigState::Committed,
            drift_summary: None,
            supervisor: SupervisorStatus {
                backend: SupervisorBackend::Pact,
                services_declared: 4,
                services_running: 4,
                services_failed: 0,
            },
            gpu_count: 4,
            gpu_healthy: 3,
            gpu_degraded: 1,
            memory_total_gb: 512.0,
            memory_avail_gb: 480.0,
        }
    }

    #[test]
    fn format_status_committed_node() {
        let status = test_status();
        let output = format_node_status(&status);
        assert!(output.contains("node042"));
        assert!(output.contains("ml-training"));
        assert!(output.contains("COMMITTED"));
        assert!(output.contains("4/4 running"));
        assert!(output.contains("GPUs: 4 total"));
        assert!(output.contains("3 healthy, 1 degraded"));
    }

    #[test]
    fn format_status_with_drift() {
        let status = NodeStatus {
            drift_summary: Some(DriftVector {
                kernel: 2.0,
                mounts: 1.0,
                files: 0.0,
                network: 0.0,
                services: 0.0,
                packages: 0.0,
                gpu: 0.0,
            }),
            config_state: ConfigState::Drifted,
            ..test_status()
        };
        let output = format_node_status(&status);
        assert!(output.contains("DRIFTED"));
        assert!(output.contains("kernel(2.0)"));
        assert!(output.contains("mounts(1.0)"));
        assert!(!output.contains("files"));
    }

    #[test]
    fn format_status_no_gpus() {
        let status = NodeStatus { gpu_count: 0, gpu_healthy: 0, gpu_degraded: 0, ..test_status() };
        let output = format_node_status(&status);
        assert!(!output.contains("GPUs"));
    }

    #[test]
    fn format_status_emergency() {
        let status = NodeStatus { config_state: ConfigState::Emergency, ..test_status() };
        let output = format_node_status(&status);
        assert!(output.contains("EMERGENCY"));
    }

    #[test]
    fn format_status_systemd_backend() {
        let status = NodeStatus {
            supervisor: SupervisorStatus {
                backend: SupervisorBackend::Systemd,
                services_declared: 2,
                services_running: 1,
                services_failed: 1,
            },
            ..test_status()
        };
        let output = format_node_status(&status);
        assert!(output.contains("systemd"));
        assert!(output.contains("1/2 running, 1 failed"));
    }

    #[test]
    fn format_gpu_health_values() {
        assert_eq!(format_gpu_health(&GpuHealth::Healthy), "healthy");
        assert_eq!(format_gpu_health(&GpuHealth::Degraded), "DEGRADED");
        assert_eq!(format_gpu_health(&GpuHealth::Failed), "FAILED");
    }

    #[test]
    fn format_drift_empty_when_no_drift() {
        let drift = DriftVector::default();
        assert!(format_drift_categories(&drift).is_empty());
    }

    #[test]
    fn format_drift_all_categories() {
        let drift = DriftVector {
            kernel: 1.0,
            mounts: 2.0,
            files: 3.0,
            network: 4.0,
            services: 5.0,
            packages: 6.0,
            gpu: 7.0,
        };
        let output = format_drift_categories(&drift);
        assert!(output.contains("kernel(1.0)"));
        assert!(output.contains("gpu(7.0)"));
    }
}
