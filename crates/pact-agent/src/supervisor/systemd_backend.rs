//! SystemdBackend — delegates service management to systemd.
//!
//! Fallback supervisor for conservative deployments where systemd
//! is the init system. Generates unit files from ServiceDecl and
//! delegates lifecycle to systemd via `systemctl`.
//!
//! In systemd mode:
//! - No pact supervision loop (systemd handles Restart=)
//! - No cgroup management (systemd manages slices)
//! - No network configuration (wickedd/NetworkManager handles it)
//! - No identity mapping (SSSD handles NSS)
//! - No hardware watchdog (systemd handles it)

use async_trait::async_trait;
use tracing::{debug, info, warn};

use pact_common::types::{RestartPolicy, ServiceDecl, ServiceState};

use super::{HealthCheckResult, ServiceManager, ServiceStatus};

/// SystemdBackend — delegates to systemd for service management.
pub struct SystemdBackend;

impl SystemdBackend {
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    /// Generate a systemd unit file content from a ServiceDecl.
    #[must_use]
    pub fn generate_unit(service: &ServiceDecl) -> String {
        let restart = match service.restart {
            RestartPolicy::Always => "always",
            RestartPolicy::OnFailure => "on-failure",
            RestartPolicy::Never => "no",
        };

        let mut unit = format!(
            "[Unit]\n\
             Description=pact-managed: {name}\n\
             After=network.target\n\
             \n\
             [Service]\n\
             Type=simple\n\
             ExecStart={binary}{args}\n\
             Restart={restart}\n\
             RestartSec={delay}\n",
            name = service.name,
            binary = service.binary,
            args = if service.args.is_empty() {
                String::new()
            } else {
                format!(" {}", service.args.join(" "))
            },
            restart = restart,
            delay = service.restart_delay_seconds,
        );

        if let Some(ref mem_max) = service.cgroup_memory_max {
            unit.push_str(&format!("MemoryMax={mem_max}\n"));
        }
        if let Some(cpu_weight) = service.cgroup_cpu_weight {
            unit.push_str(&format!("CPUWeight={cpu_weight}\n"));
        }

        unit.push_str("\n[Install]\nWantedBy=multi-user.target\n");
        unit
    }

    /// Run a systemctl command.
    #[cfg(target_os = "linux")]
    fn systemctl(args: &[&str]) -> anyhow::Result<String> {
        let output = std::process::Command::new("systemctl").args(args).output()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("systemctl {} failed: {}", args.join(" "), stderr.trim());
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    #[cfg(not(target_os = "linux"))]
    fn systemctl(args: &[&str]) -> anyhow::Result<String> {
        debug!(args = ?args, "systemctl stub (not Linux)");
        Ok(String::new())
    }
}

impl Default for SystemdBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceManager for SystemdBackend {
    async fn start(&self, service: &ServiceDecl) -> anyhow::Result<()> {
        info!(service = %service.name, "systemd: starting service");
        Self::systemctl(&["start", &format!("pact-{}.service", service.name)])?;
        Ok(())
    }

    async fn stop(&self, service: &ServiceDecl) -> anyhow::Result<()> {
        info!(service = %service.name, "systemd: stopping service");
        Self::systemctl(&["stop", &format!("pact-{}.service", service.name)])?;
        Ok(())
    }

    async fn restart(&self, service: &ServiceDecl) -> anyhow::Result<()> {
        info!(service = %service.name, "systemd: restarting service");
        Self::systemctl(&["restart", &format!("pact-{}.service", service.name)])?;
        Ok(())
    }

    async fn status(&self, service: &ServiceDecl) -> anyhow::Result<ServiceStatus> {
        let unit_name = format!("pact-{}.service", service.name);
        let output = Self::systemctl(&["is-active", &unit_name]).unwrap_or_default();
        let state = match output.trim() {
            "active" => ServiceState::Running,
            "failed" => ServiceState::Failed,
            "inactive" => ServiceState::Stopped,
            _ => ServiceState::Stopped,
        };
        Ok(ServiceStatus {
            name: service.name.clone(),
            state,
            pid: None, // systemd tracks PID internally
            restarts: 0,
            last_exit_code: None,
        })
    }

    async fn health(&self, service: &ServiceDecl) -> anyhow::Result<HealthCheckResult> {
        let status = self.status(service).await?;
        Ok(HealthCheckResult {
            healthy: status.state == ServiceState::Running,
            detail: format!("systemd state: {:?}", status.state),
        })
    }

    async fn start_all(&self, services: &[ServiceDecl]) -> anyhow::Result<()> {
        // Generate unit files, then start in order
        let mut sorted: Vec<&ServiceDecl> = services.iter().collect();
        sorted.sort_by_key(|s| s.order);

        for service in sorted {
            let unit = Self::generate_unit(service);
            let unit_path = format!("/run/systemd/transient/pact-{}.service", service.name);
            debug!(
                service = %service.name,
                unit_path = %unit_path,
                "generated unit file"
            );
            // On Linux: would write unit file and daemon-reload
            // For now: log the generated content
            debug!(unit_content = %unit, "unit file content");
            self.start(service).await?;
        }
        Ok(())
    }

    async fn stop_all(&self, services: &[ServiceDecl]) -> anyhow::Result<()> {
        let mut sorted: Vec<&ServiceDecl> = services.iter().collect();
        sorted.sort_by_key(|s| s.order);
        sorted.reverse();

        for service in sorted {
            if let Err(e) = self.stop(service).await {
                warn!(service = %service.name, "systemd stop failed: {e}");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_service() -> ServiceDecl {
        ServiceDecl {
            name: "chronyd".into(),
            binary: "/usr/sbin/chronyd".into(),
            args: vec![],
            restart: RestartPolicy::Always,
            restart_delay_seconds: 5,
            depends_on: vec![],
            order: 1,
            cgroup_memory_max: Some("128M".into()),
            cgroup_slice: None,
            cgroup_cpu_weight: Some(200),
            health_check: None,
        }
    }

    #[test]
    fn generate_unit_basic() {
        let unit = SystemdBackend::generate_unit(&test_service());
        assert!(unit.contains("[Unit]"));
        assert!(unit.contains("Description=pact-managed: chronyd"));
        assert!(unit.contains("ExecStart=/usr/sbin/chronyd"));
        assert!(unit.contains("Restart=always"));
        assert!(unit.contains("RestartSec=5"));
        assert!(unit.contains("MemoryMax=128M"));
        assert!(unit.contains("CPUWeight=200"));
    }

    #[test]
    fn generate_unit_with_args() {
        let svc = ServiceDecl {
            name: "cxi_rh".into(),
            binary: "/usr/bin/cxi_rh".into(),
            args: vec!["--device=cxi0".into()],
            restart: RestartPolicy::OnFailure,
            restart_delay_seconds: 2,
            depends_on: vec![],
            order: 3,
            cgroup_memory_max: None,
            cgroup_slice: None,
            cgroup_cpu_weight: None,
            health_check: None,
        };
        let unit = SystemdBackend::generate_unit(&svc);
        assert!(unit.contains("ExecStart=/usr/bin/cxi_rh --device=cxi0"));
        assert!(unit.contains("Restart=on-failure"));
        assert!(!unit.contains("MemoryMax"));
    }

    #[test]
    fn generate_unit_never_restart() {
        let svc = ServiceDecl { restart: RestartPolicy::Never, ..test_service() };
        let unit = SystemdBackend::generate_unit(&svc);
        assert!(unit.contains("Restart=no"));
    }

    #[tokio::test]
    async fn systemd_backend_status_stub() {
        let backend = SystemdBackend::new();
        let status = backend.status(&test_service()).await.unwrap();
        // On non-Linux: systemctl returns empty → Stopped
        assert_eq!(status.state, ServiceState::Stopped);
    }

    #[tokio::test]
    async fn systemd_backend_health_stub() {
        let backend = SystemdBackend::new();
        let health = backend.health(&test_service()).await.unwrap();
        // On non-Linux: not running → not healthy
        assert!(!health.healthy);
    }
}
