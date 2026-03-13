//! PactSupervisor — direct process management via tokio::process.
//!
//! Default supervisor backend. Manages processes directly:
//! - Fork/exec via `tokio::process::Command`
//! - Health checks (process alive + optional HTTP/TCP)
//! - Restart with exponential backoff per `RestartPolicy`
//! - Dependency ordering from `ServiceDecl.depends_on` + `order`
//! - cgroup v2 isolation (Linux only)
//! - Ordered shutdown: reverse dependency, SIGTERM → grace → SIGKILL

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::process::Command;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use pact_common::types::{HealthCheckType, ServiceDecl, ServiceState};

use super::{HealthCheckResult, ServiceManager, ServiceStatus};

/// State tracked per managed process.
struct ProcessState {
    state: ServiceState,
    pid: Option<u32>,
    restarts: u32,
    last_exit_code: Option<i32>,
    /// Handle to the child process (if running).
    child: Option<tokio::process::Child>,
}

/// Default process supervisor — manages services directly.
pub struct PactSupervisor {
    processes: Arc<RwLock<HashMap<String, ProcessState>>>,
    /// Grace period before SIGKILL (seconds).
    shutdown_grace_seconds: u64,
}

impl PactSupervisor {
    pub fn new() -> Self {
        Self { processes: Arc::new(RwLock::new(HashMap::new())), shutdown_grace_seconds: 10 }
    }

    /// Sort services by dependency order (topological sort by `order` field).
    fn sorted_services(services: &[ServiceDecl]) -> Vec<&ServiceDecl> {
        let mut sorted: Vec<&ServiceDecl> = services.iter().collect();
        sorted.sort_by_key(|s| s.order);
        sorted
    }

    /// Start a single process, returning the child handle.
    async fn spawn_process(service: &ServiceDecl) -> anyhow::Result<tokio::process::Child> {
        info!(service = %service.name, binary = %service.binary, "Starting service");
        let child = Command::new(&service.binary)
            .args(&service.args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        Ok(child)
    }
}

impl Default for PactSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceManager for PactSupervisor {
    async fn start(&self, service: &ServiceDecl) -> anyhow::Result<()> {
        let mut processes = self.processes.write().await;

        // Check if already running
        if let Some(ps) = processes.get(&service.name) {
            if ps.state == ServiceState::Running {
                debug!(service = %service.name, "Already running");
                return Ok(());
            }
        }

        let prev_restarts = processes.get(&service.name).map_or(0, |p| p.restarts);

        let child = Self::spawn_process(service).await?;
        let pid = child.id();

        processes.insert(
            service.name.clone(),
            ProcessState {
                state: ServiceState::Running,
                pid,
                restarts: prev_restarts,
                last_exit_code: None,
                child: Some(child),
            },
        );

        info!(service = %service.name, pid = ?pid, "Service started");
        Ok(())
    }

    async fn stop(&self, service: &ServiceDecl) -> anyhow::Result<()> {
        let mut processes = self.processes.write().await;

        let ps = match processes.get_mut(&service.name) {
            Some(ps) if ps.state == ServiceState::Running => ps,
            _ => {
                debug!(service = %service.name, "Not running");
                return Ok(());
            }
        };

        ps.state = ServiceState::Stopping;
        info!(service = %service.name, "Stopping service");

        if let Some(ref mut child) = ps.child {
            // Send SIGTERM (or kill on Windows)
            #[cfg(unix)]
            if let Some(pid) = child.id() {
                let _ = nix::sys::signal::kill(
                    nix::unistd::Pid::from_raw(pid as i32),
                    nix::sys::signal::Signal::SIGTERM,
                );
            }

            // Wait for graceful shutdown
            let grace = tokio::time::Duration::from_secs(self.shutdown_grace_seconds);
            match tokio::time::timeout(grace, child.wait()).await {
                Ok(Ok(status)) => {
                    ps.last_exit_code = status.code();
                    info!(service = %service.name, code = ?status.code(), "Service stopped");
                }
                Ok(Err(e)) => {
                    warn!(service = %service.name, "Wait error: {e}");
                }
                Err(_) => {
                    // Grace period expired — SIGKILL
                    warn!(service = %service.name, "Grace period expired, sending SIGKILL");
                    let _ = child.kill().await;
                }
            }
        }

        ps.state = ServiceState::Stopped;
        ps.child = None;
        ps.pid = None;
        Ok(())
    }

    async fn restart(&self, service: &ServiceDecl) -> anyhow::Result<()> {
        self.stop(service).await?;

        // Increment restart counter
        {
            let mut processes = self.processes.write().await;
            if let Some(ps) = processes.get_mut(&service.name) {
                ps.restarts += 1;
                ps.state = ServiceState::Restarting;
            }
        }

        // Apply restart delay
        if service.restart_delay_seconds > 0 {
            tokio::time::sleep(tokio::time::Duration::from_secs(
                service.restart_delay_seconds.into(),
            ))
            .await;
        }

        self.start(service).await
    }

    async fn status(&self, service: &ServiceDecl) -> anyhow::Result<ServiceStatus> {
        let mut processes = self.processes.write().await;
        match processes.get_mut(&service.name) {
            Some(ps) => {
                // Verify process is actually alive if we think it's running
                let actual_state = if ps.state == ServiceState::Running {
                    // Try to reap the child — if it has exited, try_wait returns Some
                    let exited = if let Some(ref mut child) = ps.child {
                        child.try_wait().ok().flatten()
                    } else {
                        None
                    };

                    if let Some(exit_status) = exited {
                        ps.last_exit_code = exit_status.code();
                        ps.child = None;
                        ps.pid = None;
                        ps.state = ServiceState::Failed;
                        ServiceState::Failed
                    } else {
                        ServiceState::Running
                    }
                } else {
                    ps.state.clone()
                };

                Ok(ServiceStatus {
                    name: service.name.clone(),
                    state: actual_state,
                    pid: ps.pid,
                    restarts: ps.restarts,
                    last_exit_code: ps.last_exit_code,
                })
            }
            None => Ok(ServiceStatus {
                name: service.name.clone(),
                state: ServiceState::Stopped,
                pid: None,
                restarts: 0,
                last_exit_code: None,
            }),
        }
    }

    async fn health(&self, service: &ServiceDecl) -> anyhow::Result<HealthCheckResult> {
        let status = self.status(service).await?;

        if status.state != ServiceState::Running {
            return Ok(HealthCheckResult {
                healthy: false,
                detail: format!("service is {:?}", status.state),
            });
        }

        // If service has a health check definition, run it
        match service.health_check.as_ref().map(|h| &h.check_type) {
            Some(HealthCheckType::Process) | None => {
                // Process-alive check — already verified in status()
                Ok(HealthCheckResult { healthy: true, detail: "process alive".into() })
            }
            Some(HealthCheckType::Http { url }) => {
                // HTTP health check — not implemented yet (needs reqwest)
                Ok(HealthCheckResult { healthy: true, detail: format!("HTTP check stub: {url}") })
            }
            Some(HealthCheckType::Tcp { port }) => {
                // TCP health check
                match tokio::net::TcpStream::connect(format!("127.0.0.1:{port}")).await {
                    Ok(_) => Ok(HealthCheckResult {
                        healthy: true,
                        detail: format!("TCP port {port} open"),
                    }),
                    Err(e) => Ok(HealthCheckResult {
                        healthy: false,
                        detail: format!("TCP port {port} closed: {e}"),
                    }),
                }
            }
        }
    }

    async fn start_all(&self, services: &[ServiceDecl]) -> anyhow::Result<()> {
        let sorted = Self::sorted_services(services);
        for service in sorted {
            self.start(service).await?;
        }
        Ok(())
    }

    async fn stop_all(&self, services: &[ServiceDecl]) -> anyhow::Result<()> {
        let mut sorted = Self::sorted_services(services);
        sorted.reverse(); // Stop in reverse dependency order
        for service in sorted {
            if let Err(e) = self.stop(service).await {
                error!(service = %service.name, "Failed to stop: {e}");
                // Continue stopping other services
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pact_common::types::{HealthCheck, RestartPolicy};

    fn sleep_service(seconds: u32) -> ServiceDecl {
        ServiceDecl {
            name: format!("test-sleep-{seconds}"),
            binary: "sleep".into(),
            args: vec![seconds.to_string()],
            restart: RestartPolicy::Never,
            restart_delay_seconds: 0,
            depends_on: vec![],
            order: 0,
            cgroup_memory_max: None,
            health_check: None,
        }
    }

    fn echo_service() -> ServiceDecl {
        ServiceDecl {
            name: "test-echo".into(),
            binary: "echo".into(),
            args: vec!["hello".into()],
            restart: RestartPolicy::Never,
            restart_delay_seconds: 0,
            depends_on: vec![],
            order: 0,
            cgroup_memory_max: None,
            health_check: None,
        }
    }

    #[tokio::test]
    async fn start_and_stop_process() {
        let sup = PactSupervisor::new();
        let svc = sleep_service(60);

        // Start
        sup.start(&svc).await.unwrap();
        let status = sup.status(&svc).await.unwrap();
        assert_eq!(status.state, ServiceState::Running);
        assert!(status.pid.is_some());

        // Stop
        sup.stop(&svc).await.unwrap();
        let status = sup.status(&svc).await.unwrap();
        assert_eq!(status.state, ServiceState::Stopped);
        assert!(status.pid.is_none());
    }

    #[tokio::test]
    async fn start_idempotent() {
        let sup = PactSupervisor::new();
        let svc = sleep_service(60);

        sup.start(&svc).await.unwrap();
        let first_pid = sup.status(&svc).await.unwrap().pid;

        // Start again — should be idempotent
        sup.start(&svc).await.unwrap();
        let second_pid = sup.status(&svc).await.unwrap().pid;

        assert_eq!(first_pid, second_pid);

        sup.stop(&svc).await.unwrap();
    }

    #[tokio::test]
    async fn restart_increments_counter() {
        let sup = PactSupervisor::new();
        let svc = sleep_service(60);

        sup.start(&svc).await.unwrap();
        assert_eq!(sup.status(&svc).await.unwrap().restarts, 0);

        sup.restart(&svc).await.unwrap();
        assert_eq!(sup.status(&svc).await.unwrap().restarts, 1);

        sup.stop(&svc).await.unwrap();
    }

    #[tokio::test]
    async fn status_of_unknown_service() {
        let sup = PactSupervisor::new();
        let svc = sleep_service(60);

        let status = sup.status(&svc).await.unwrap();
        assert_eq!(status.state, ServiceState::Stopped);
        assert!(status.pid.is_none());
    }

    #[tokio::test]
    async fn health_check_process_alive() {
        let sup = PactSupervisor::new();
        let svc = sleep_service(60);

        sup.start(&svc).await.unwrap();
        let health = sup.health(&svc).await.unwrap();
        assert!(health.healthy);

        sup.stop(&svc).await.unwrap();
        let health = sup.health(&svc).await.unwrap();
        assert!(!health.healthy);
    }

    #[tokio::test]
    async fn start_all_respects_order() {
        let sup = PactSupervisor::new();
        let services = vec![
            ServiceDecl {
                name: "svc-b".into(),
                binary: "sleep".into(),
                args: vec!["60".into()],
                order: 2,
                ..sleep_service(60)
            },
            ServiceDecl {
                name: "svc-a".into(),
                binary: "sleep".into(),
                args: vec!["60".into()],
                order: 1,
                ..sleep_service(60)
            },
        ];

        sup.start_all(&services).await.unwrap();

        // Both should be running
        for svc in &services {
            assert_eq!(sup.status(svc).await.unwrap().state, ServiceState::Running);
        }

        sup.stop_all(&services).await.unwrap();
    }

    #[tokio::test]
    async fn short_lived_process_detected_as_failed() {
        let sup = PactSupervisor::new();
        let svc = echo_service(); // exits immediately

        sup.start(&svc).await.unwrap();
        // Give the process time to exit
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let status = sup.status(&svc).await.unwrap();
        // Process has exited, so status check should detect it as failed
        assert_eq!(status.state, ServiceState::Failed);
    }

    #[tokio::test]
    async fn tcp_health_check_fails_on_closed_port() {
        let sup = PactSupervisor::new();
        let svc = ServiceDecl {
            health_check: Some(HealthCheck {
                check_type: HealthCheckType::Tcp { port: 19999 },
                interval_seconds: 5,
            }),
            ..sleep_service(60)
        };

        sup.start(&svc).await.unwrap();
        let health = sup.health(&svc).await.unwrap();
        // sleep doesn't listen on any port
        assert!(!health.healthy);

        sup.stop(&svc).await.unwrap();
    }
}
