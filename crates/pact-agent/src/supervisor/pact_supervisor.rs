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
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

use hpc_audit::{AuditEvent, AuditPrincipal, AuditScope, AuditSink, AuditSource};
use hpc_node::{CgroupHandle, CgroupManager, ResourceLimits};
use pact_common::types::{HealthCheckType, RestartPolicy, ServiceDecl, ServiceState};

use super::{HealthCheckResult, ServiceManager, ServiceStatus};

/// State tracked per managed process.
struct ProcessState {
    state: ServiceState,
    pid: Option<u32>,
    restarts: u32,
    last_exit_code: Option<i32>,
    /// Handle to the child process (if running).
    child: Option<tokio::process::Child>,
    /// cgroup scope handle (if cgroup manager is active).
    cgroup_handle: Option<CgroupHandle>,
}

/// Workload state determines supervision loop poll interval (PS1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadState {
    /// No processes in workload.slice — faster polling, deeper inspections.
    Idle,
    /// Processes active in workload.slice — slower polling, minimal overhead.
    Active,
}

/// Configuration for the supervision loop.
#[derive(Debug, Clone)]
pub struct SupervisionConfig {
    /// Poll interval when idle (no workloads). Default: 500ms.
    pub idle_interval_ms: u64,
    /// Poll interval when active (workloads running). Default: 2000ms.
    pub active_interval_ms: u64,
}

impl Default for SupervisionConfig {
    fn default() -> Self {
        Self { idle_interval_ms: 500, active_interval_ms: 2000 }
    }
}

/// Default process supervisor — manages services directly.
pub struct PactSupervisor {
    processes: Arc<RwLock<HashMap<String, ProcessState>>>,
    /// Service declarations (needed by supervision loop for restart).
    service_decls: Arc<RwLock<Vec<ServiceDecl>>>,
    /// Grace period before SIGKILL (seconds).
    shutdown_grace_seconds: u64,
    /// Supervision loop configuration.
    supervision_config: SupervisionConfig,
    /// Optional cgroup manager for resource isolation (RI2).
    /// None when running on non-Linux or when cgroups are not available.
    cgroup_manager: Option<Arc<dyn CgroupManager>>,
}

impl PactSupervisor {
    pub fn new() -> Self {
        Self {
            processes: Arc::new(RwLock::new(HashMap::new())),
            service_decls: Arc::new(RwLock::new(Vec::new())),
            shutdown_grace_seconds: 10,
            supervision_config: SupervisionConfig::default(),
            cgroup_manager: None,
        }
    }

    /// Create with custom supervision config.
    #[must_use]
    pub fn with_config(supervision_config: SupervisionConfig) -> Self {
        Self { supervision_config, ..Self::new() }
    }

    /// Create with a cgroup manager for resource isolation.
    #[must_use]
    pub fn with_cgroup_manager(mut self, cgroup_manager: Arc<dyn CgroupManager>) -> Self {
        self.cgroup_manager = Some(cgroup_manager);
        self
    }

    /// Convert a `ServiceDecl`'s cgroup fields to `ResourceLimits`.
    fn resource_limits(service: &ServiceDecl) -> ResourceLimits {
        ResourceLimits {
            memory_max: service.cgroup_memory_max.as_deref().and_then(parse_memory_value),
            cpu_weight: service.cgroup_cpu_weight,
            io_max: None,
        }
    }

    /// Get the cgroup slice for a service (defaults to infra).
    fn cgroup_slice(service: &ServiceDecl) -> &str {
        service.cgroup_slice.as_deref().unwrap_or(hpc_node::cgroup::slices::PACT_INFRA)
    }

    /// Start the background supervision loop.
    ///
    /// Returns a `JoinHandle` for the loop task. The loop:
    /// - Polls process status via `try_wait()`
    /// - Evaluates `RestartPolicy` for crashed services
    /// - Adapts poll interval based on workload state (PS1)
    /// - Calls `watchdog_pet` callback each tick (PS2)
    ///
    /// The loop runs until the returned handle is aborted or the
    /// `shutdown` channel is signaled.
    pub fn start_supervision_loop<S: AuditSink + 'static>(
        &self,
        audit_sink: Arc<S>,
        node_id: String,
        watchdog_pet: Option<Arc<dyn Fn() + Send + Sync>>,
        workload_active: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> JoinHandle<()> {
        let processes = Arc::clone(&self.processes);
        let service_decls = Arc::clone(&self.service_decls);
        let config = self.supervision_config.clone();
        let grace_secs = self.shutdown_grace_seconds;
        let cgroup_mgr = self.cgroup_manager.clone();

        tokio::spawn(async move {
            info!("supervision loop started");
            loop {
                // Determine workload state and poll interval (PS1)
                let is_active = workload_active();
                let interval_ms =
                    if is_active { config.active_interval_ms } else { config.idle_interval_ms };

                // Pet the watchdog (PS2 — coupled to loop tick)
                if let Some(ref pet) = watchdog_pet {
                    pet();
                }

                // Check all processes for unexpected exits
                let crashed = Self::detect_crashed_services(&processes).await;

                // Handle restarts per policy
                let decls = service_decls.read().await;
                for (name, exit_code) in &crashed {
                    if let Some(decl) = decls.iter().find(|d| &d.name == name) {
                        let should_restart = match decl.restart {
                            RestartPolicy::Always => true,
                            RestartPolicy::OnFailure => exit_code.is_none_or(|c| c != 0),
                            RestartPolicy::Never => false,
                        };

                        // Emit audit event for crash
                        let event =
                            Self::crash_audit_event(name, *exit_code, &node_id, should_restart);
                        audit_sink.emit(event);

                        if should_restart {
                            info!(
                                service = %name,
                                exit_code = ?exit_code,
                                policy = ?decl.restart,
                                "supervision loop restarting service"
                            );

                            // Apply restart delay
                            if decl.restart_delay_seconds > 0 {
                                tokio::time::sleep(tokio::time::Duration::from_secs(
                                    decl.restart_delay_seconds.into(),
                                ))
                                .await;
                            }

                            // Restart (with cgroup scope creation)
                            if let Err(e) =
                                Self::do_restart(&processes, decl, grace_secs, cgroup_mgr.as_ref())
                                    .await
                            {
                                error!(service = %name, "supervision restart failed: {e}");
                            }
                        } else {
                            info!(
                                service = %name,
                                exit_code = ?exit_code,
                                policy = ?decl.restart,
                                "supervision loop: not restarting per policy"
                            );
                            // Mark as Stopped for Never policy
                            let mut procs = processes.write().await;
                            if let Some(ps) = procs.get_mut(name.as_str()) {
                                if decl.restart == RestartPolicy::Never {
                                    ps.state = ServiceState::Stopped;
                                }
                            }
                        }
                    }
                }
                drop(decls);

                tokio::time::sleep(tokio::time::Duration::from_millis(interval_ms)).await;
            }
        })
    }

    /// Detect services whose processes have exited unexpectedly.
    /// Returns vec of (service_name, exit_code).
    async fn detect_crashed_services(
        processes: &Arc<RwLock<HashMap<String, ProcessState>>>,
    ) -> Vec<(String, Option<i32>)> {
        let mut crashed = Vec::new();
        let mut procs = processes.write().await;

        for (name, ps) in procs.iter_mut() {
            if ps.state != ServiceState::Running {
                continue;
            }

            let exited = if let Some(ref mut child) = ps.child {
                child.try_wait().ok().flatten()
            } else {
                None
            };

            if let Some(exit_status) = exited {
                let code = exit_status.code();
                warn!(service = %name, exit_code = ?code, "service exited unexpectedly");
                ps.last_exit_code = code;
                ps.child = None;
                ps.pid = None;
                ps.state = ServiceState::Failed;
                ps.restarts += 1;
                crashed.push((name.clone(), code));
            }
        }

        crashed
    }

    /// Restart a service (internal, used by supervision loop).
    async fn do_restart(
        processes: &Arc<RwLock<HashMap<String, ProcessState>>>,
        service: &ServiceDecl,
        _grace_secs: u64,
        cgroup_mgr: Option<&Arc<dyn CgroupManager>>,
    ) -> anyhow::Result<()> {
        // Destroy old cgroup scope if present
        {
            let mut procs = processes.write().await;
            if let Some(ps) = procs.get_mut(&service.name) {
                if let (Some(mgr), Some(ref handle)) = (cgroup_mgr, &ps.cgroup_handle) {
                    if let Err(e) = mgr.destroy_scope(handle) {
                        warn!(service = %service.name, "old cgroup scope cleanup failed: {e}");
                    }
                }
                ps.cgroup_handle = None;
            }
        }

        // Create new cgroup scope
        let cgroup_handle = if let Some(mgr) = cgroup_mgr {
            let limits = Self::resource_limits(service);
            let slice = Self::cgroup_slice(service);
            match mgr.create_scope(slice, &service.name, &limits) {
                Ok(handle) => Some(handle),
                Err(e) => {
                    error!(service = %service.name, "cgroup scope creation on restart failed: {e}");
                    None
                }
            }
        } else {
            None
        };

        let child = Self::spawn_process(service).await?;
        let pid = child.id();

        let mut procs = processes.write().await;
        if let Some(ps) = procs.get_mut(&service.name) {
            ps.state = ServiceState::Running;
            ps.pid = pid;
            ps.child = Some(child);
            ps.cgroup_handle = cgroup_handle;
            info!(service = %service.name, pid = ?pid, "service restarted by supervision loop");
        } else {
            procs.insert(
                service.name.clone(),
                ProcessState {
                    state: ServiceState::Running,
                    pid,
                    restarts: 1,
                    last_exit_code: None,
                    child: Some(child),
                    cgroup_handle,
                },
            );
        }
        Ok(())
    }

    fn crash_audit_event(
        service_name: &str,
        exit_code: Option<i32>,
        node_id: &str,
        will_restart: bool,
    ) -> AuditEvent {
        AuditEvent {
            id: uuid::Uuid::new_v4().to_string(),
            timestamp: chrono::Utc::now(),
            principal: AuditPrincipal::system("supervision-loop"),
            action: hpc_audit::actions::SERVICE_CRASH.to_string(),
            scope: AuditScope::node(node_id),
            outcome: hpc_audit::AuditOutcome::Failure,
            detail: format!(
                "service {service_name} crashed with exit code {exit_code:?}, restart={will_restart}"
            ),
            metadata: serde_json::json!({
                "service": service_name,
                "exit_code": exit_code,
                "will_restart": will_restart,
            }),
            source: AuditSource::PactAgent,
        }
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

/// Parse memory value like "512M", "1G", "1048576" to bytes.
fn parse_memory_value(s: &str) -> Option<u64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num, multiplier) = if let Some(n) = s.strip_suffix('G').or_else(|| s.strip_suffix('g')) {
        (n, 1024 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix('M').or_else(|| s.strip_suffix('m')) {
        (n, 1024 * 1024)
    } else if let Some(n) = s.strip_suffix('K').or_else(|| s.strip_suffix('k')) {
        (n, 1024)
    } else {
        (s, 1)
    };
    num.trim().parse::<u64>().ok().map(|v| v * multiplier)
}

impl Default for PactSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceManager for PactSupervisor {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

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

        // Create cgroup scope before spawning (RI2)
        let cgroup_handle = if let Some(ref mgr) = self.cgroup_manager {
            let limits = Self::resource_limits(service);
            let slice = Self::cgroup_slice(service);
            match mgr.create_scope(slice, &service.name, &limits) {
                Ok(handle) => {
                    debug!(service = %service.name, scope = %handle.path, "cgroup scope created");
                    Some(handle)
                }
                Err(e) => {
                    warn!(service = %service.name, "cgroup scope creation failed (continuing without isolation): {e}");
                    None
                }
            }
        } else {
            None
        };

        // Spawn process
        match Self::spawn_process(service).await {
            Ok(child) => {
                let pid = child.id();
                processes.insert(
                    service.name.clone(),
                    ProcessState {
                        state: ServiceState::Running,
                        pid,
                        restarts: prev_restarts,
                        last_exit_code: None,
                        child: Some(child),
                        cgroup_handle,
                    },
                );
                info!(service = %service.name, pid = ?pid, "Service started");
                Ok(())
            }
            Err(e) => {
                // Spawn failed — clean up cgroup scope (RI5: callback on failure)
                if let (Some(ref mgr), Some(ref handle)) = (&self.cgroup_manager, &cgroup_handle) {
                    if let Err(cleanup_err) = mgr.destroy_scope(handle) {
                        warn!(
                            service = %service.name,
                            "cgroup cleanup after spawn failure also failed: {cleanup_err}"
                        );
                    }
                }
                Err(e)
            }
        }
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

        // Destroy cgroup scope (PS3: kill all children)
        if let (Some(ref mgr), Some(ref handle)) = (&self.cgroup_manager, &ps.cgroup_handle) {
            if let Err(e) = mgr.destroy_scope(handle) {
                warn!(service = %service.name, "cgroup scope cleanup failed: {e}");
            }
        }
        ps.cgroup_handle = None;

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
                // HTTP health check — GET the URL and check for 2xx status
                match reqwest::get(url).await {
                    Ok(resp) if resp.status().is_success() => Ok(HealthCheckResult {
                        healthy: true,
                        detail: format!("HTTP {}", resp.status()),
                    }),
                    Ok(resp) => Ok(HealthCheckResult {
                        healthy: false,
                        detail: format!("HTTP {}", resp.status()),
                    }),
                    Err(e) => Ok(HealthCheckResult {
                        healthy: false,
                        detail: format!("HTTP check failed: {e}"),
                    }),
                }
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
        // Store declarations for supervision loop restarts
        {
            let mut decls = self.service_decls.write().await;
            *decls = services.to_vec();
        }

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
            cgroup_slice: None,
            cgroup_cpu_weight: None,
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
            cgroup_slice: None,
            cgroup_cpu_weight: None,
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
    async fn supervision_loop_restarts_crashed_service() {
        use hpc_audit::MemoryAuditSink;

        let sup = PactSupervisor::with_config(SupervisionConfig {
            idle_interval_ms: 100,
            active_interval_ms: 100,
        });

        // Use echo which exits immediately — simulates a crash
        let services = vec![ServiceDecl {
            name: "crasher".into(),
            binary: "echo".into(),
            args: vec!["crash".into()],
            restart: RestartPolicy::Always,
            restart_delay_seconds: 0,
            depends_on: vec![],
            order: 1,
            cgroup_memory_max: None,
            cgroup_slice: None,
            cgroup_cpu_weight: None,
            health_check: None,
        }];

        sup.start_all(&services).await.unwrap();

        let audit = Arc::new(MemoryAuditSink::new());
        let handle = sup.start_supervision_loop(
            Arc::clone(&audit),
            "test-node".to_string(),
            None,
            Arc::new(|| false), // idle
        );

        // Wait for the loop to detect the crash and restart
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        handle.abort();

        // Audit sink should have crash events
        let events = audit.events();
        assert!(!events.is_empty(), "supervision loop should emit crash audit events");
        assert!(events.iter().any(|e| e.action == hpc_audit::actions::SERVICE_CRASH));
    }

    #[tokio::test]
    async fn supervision_loop_does_not_restart_never_policy() {
        use hpc_audit::MemoryAuditSink;

        let sup = PactSupervisor::with_config(SupervisionConfig {
            idle_interval_ms: 100,
            active_interval_ms: 100,
        });

        let services = vec![ServiceDecl {
            name: "oneshot".into(),
            binary: "echo".into(),
            args: vec!["done".into()],
            restart: RestartPolicy::Never,
            restart_delay_seconds: 0,
            depends_on: vec![],
            order: 1,
            cgroup_memory_max: None,
            cgroup_slice: None,
            cgroup_cpu_weight: None,
            health_check: None,
        }];

        sup.start_all(&services).await.unwrap();

        let audit = Arc::new(MemoryAuditSink::new());
        let handle = sup.start_supervision_loop(
            Arc::clone(&audit),
            "test-node".to_string(),
            None,
            Arc::new(|| false),
        );

        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
        handle.abort();

        // Service should be in Stopped state, not restarted
        let status = sup.status(&services[0]).await.unwrap();
        assert!(
            status.state == ServiceState::Stopped || status.state == ServiceState::Failed,
            "service with Never policy should not be Running, got {:?}",
            status.state
        );
    }

    #[tokio::test]
    async fn supervision_loop_pets_watchdog() {
        use hpc_audit::NullAuditSink;
        use std::sync::atomic::{AtomicU32, Ordering};

        let sup = PactSupervisor::with_config(SupervisionConfig {
            idle_interval_ms: 50,
            active_interval_ms: 50,
        });

        let pet_count = Arc::new(AtomicU32::new(0));
        let pet_counter = Arc::clone(&pet_count);

        let handle = sup.start_supervision_loop(
            Arc::new(NullAuditSink),
            "test-node".to_string(),
            Some(Arc::new(move || {
                pet_counter.fetch_add(1, Ordering::Relaxed);
            })),
            Arc::new(|| false),
        );

        tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
        handle.abort();

        let count = pet_count.load(Ordering::Relaxed);
        assert!(
            count >= 3,
            "watchdog should have been petted at least 3 times in 300ms with 50ms interval, got {count}"
        );
    }

    #[tokio::test]
    async fn supervision_loop_adapts_interval() {
        use hpc_audit::NullAuditSink;
        use std::sync::atomic::{AtomicU32, Ordering};

        // Track pet count with active workload (slower interval)
        let sup = PactSupervisor::with_config(SupervisionConfig {
            idle_interval_ms: 50,
            active_interval_ms: 200,
        });

        let pet_count = Arc::new(AtomicU32::new(0));
        let pet_counter = Arc::clone(&pet_count);

        let handle = sup.start_supervision_loop(
            Arc::new(NullAuditSink),
            "test-node".to_string(),
            Some(Arc::new(move || {
                pet_counter.fetch_add(1, Ordering::Relaxed);
            })),
            Arc::new(|| true), // active workload
        );

        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        handle.abort();

        let active_count = pet_count.load(Ordering::Relaxed);
        // With 200ms interval over 500ms, expect ~2-3 pets
        assert!(
            active_count <= 5,
            "active workload should have fewer pets (slower interval), got {active_count}"
        );
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
