//! Process supervisor — manages service lifecycle on the node.
//!
//! Two implementations:
//! - `PactSupervisor` (default): direct process management via tokio + cgroup v2
//! - `SystemdBackend` (feature `systemd`): delegates to systemd via D-Bus
//!
//! The `ServiceManager` trait provides the common interface.

mod pact_supervisor;
mod systemd_backend;

pub use pact_supervisor::{PactSupervisor, SupervisionConfig, WorkloadState};
pub use systemd_backend::SystemdBackend;

use async_trait::async_trait;
use pact_common::types::{ServiceDecl, ServiceState};

/// Status of a managed service.
#[derive(Debug, Clone)]
pub struct ServiceStatus {
    pub name: String,
    pub state: ServiceState,
    pub pid: Option<u32>,
    pub restarts: u32,
    pub last_exit_code: Option<i32>,
}

/// Result of a health check.
#[derive(Debug, Clone)]
pub struct HealthCheckResult {
    pub healthy: bool,
    pub detail: String,
}

/// Trait for process lifecycle management.
#[async_trait]
pub trait ServiceManager: Send + Sync {
    /// Downcast support for accessing concrete type (e.g., PactSupervisor).
    fn as_any(&self) -> &dyn std::any::Any;

    /// Start a service. If already running, returns Ok.
    async fn start(&self, service: &ServiceDecl) -> anyhow::Result<()>;

    /// Stop a service. SIGTERM → grace period → SIGKILL.
    async fn stop(&self, service: &ServiceDecl) -> anyhow::Result<()>;

    /// Restart a service (stop + start).
    async fn restart(&self, service: &ServiceDecl) -> anyhow::Result<()>;

    /// Get current status of a service.
    async fn status(&self, service: &ServiceDecl) -> anyhow::Result<ServiceStatus>;

    /// Run health check for a service.
    async fn health(&self, service: &ServiceDecl) -> anyhow::Result<HealthCheckResult>;

    /// Start all services in dependency order.
    async fn start_all(&self, services: &[ServiceDecl]) -> anyhow::Result<()>;

    /// Stop all services in reverse dependency order.
    async fn stop_all(&self, services: &[ServiceDecl]) -> anyhow::Result<()>;
}
