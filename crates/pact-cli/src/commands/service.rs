//! `pact service` — service management commands.
//!
//! Subcommands: status, restart, logs.
//! Service management operates through the shell exec path (whitelisted).

use pact_common::types::{ServiceState, ServiceStatusInfo};

/// Format service status for display.
pub fn format_service_status(services: &[ServiceStatusInfo]) -> String {
    if services.is_empty() {
        return "(no services)".to_string();
    }

    let mut lines = vec![format!(
        "{:<25} {:<12} {:>8} {:>10} {:>8}",
        "SERVICE", "STATE", "PID", "UPTIME", "RESTARTS"
    )];

    for svc in services {
        lines.push(format!(
            "{:<25} {:<12} {:>8} {:>10} {:>8}",
            svc.name,
            format_service_state(&svc.state),
            svc.pid,
            format_uptime(svc.uptime_seconds),
            svc.restart_count,
        ));
    }

    lines.join("\n")
}

/// Format a single service status line.
pub fn format_single_service(svc: &ServiceStatusInfo) -> String {
    format!(
        "{}: {} (PID {}, uptime {}, {} restarts)",
        svc.name,
        format_service_state(&svc.state),
        svc.pid,
        format_uptime(svc.uptime_seconds),
        svc.restart_count,
    )
}

fn format_service_state(state: &ServiceState) -> &'static str {
    match state {
        ServiceState::Starting => "starting",
        ServiceState::Running => "running",
        ServiceState::Stopping => "stopping",
        ServiceState::Stopped => "stopped",
        ServiceState::Failed => "FAILED",
        ServiceState::Restarting => "restarting",
    }
}

fn format_uptime(seconds: u64) -> String {
    if seconds < 60 {
        format!("{}s", seconds)
    } else if seconds < 3600 {
        format!("{}m{}s", seconds / 60, seconds % 60)
    } else if seconds < 86400 {
        format!("{}h{}m", seconds / 3600, (seconds % 3600) / 60)
    } else {
        format!("{}d{}h", seconds / 86400, (seconds % 86400) / 3600)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_services() -> Vec<ServiceStatusInfo> {
        vec![
            ServiceStatusInfo {
                name: "chronyd".into(),
                state: ServiceState::Running,
                pid: 1234,
                uptime_seconds: 86400,
                restart_count: 0,
            },
            ServiceStatusInfo {
                name: "nvidia-persistenced".into(),
                state: ServiceState::Running,
                pid: 1235,
                uptime_seconds: 86300,
                restart_count: 1,
            },
            ServiceStatusInfo {
                name: "lattice-node-agent".into(),
                state: ServiceState::Failed,
                pid: 0,
                uptime_seconds: 0,
                restart_count: 5,
            },
        ]
    }

    #[test]
    fn format_service_status_table() {
        let output = format_service_status(&test_services());
        assert!(output.contains("chronyd"));
        assert!(output.contains("nvidia-persistenced"));
        assert!(output.contains("lattice-node-agent"));
        assert!(output.contains("running"));
        assert!(output.contains("FAILED"));
        assert!(output.contains("SERVICE")); // header
    }

    #[test]
    fn format_service_status_empty() {
        assert_eq!(format_service_status(&[]), "(no services)");
    }

    #[test]
    fn format_single_service_running() {
        let svc = ServiceStatusInfo {
            name: "chronyd".into(),
            state: ServiceState::Running,
            pid: 1234,
            uptime_seconds: 3661,
            restart_count: 0,
        };
        let output = format_single_service(&svc);
        assert!(output.contains("chronyd: running"));
        assert!(output.contains("PID 1234"));
        assert!(output.contains("1h1m"));
        assert!(output.contains("0 restarts"));
    }

    #[test]
    fn format_uptime_seconds() {
        assert_eq!(format_uptime(30), "30s");
    }

    #[test]
    fn format_uptime_minutes() {
        assert_eq!(format_uptime(125), "2m5s");
    }

    #[test]
    fn format_uptime_hours() {
        assert_eq!(format_uptime(7261), "2h1m");
    }

    #[test]
    fn format_uptime_days() {
        assert_eq!(format_uptime(90000), "1d1h");
    }

    #[test]
    fn format_service_state_all_variants() {
        assert_eq!(format_service_state(&ServiceState::Starting), "starting");
        assert_eq!(format_service_state(&ServiceState::Running), "running");
        assert_eq!(format_service_state(&ServiceState::Stopping), "stopping");
        assert_eq!(format_service_state(&ServiceState::Stopped), "stopped");
        assert_eq!(format_service_state(&ServiceState::Failed), "FAILED");
        assert_eq!(format_service_state(&ServiceState::Restarting), "restarting");
    }
}
