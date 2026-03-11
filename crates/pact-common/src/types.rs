//! Core domain types for pact.
//!
//! All public types derive `Debug, Clone, Serialize, Deserialize` where possible.
//! Algebraic types (enums) for state, not strings.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Semantic type aliases for clarity.
pub type NodeId = String;
pub type VClusterId = String;
pub type EntrySeq = u64;

/// Configuration state of a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConfigState {
    /// Observing only, no enforcement.
    ObserveOnly,
    /// All config committed, no drift.
    Committed,
    /// Drift detected, within commit window.
    Drifted,
    /// Actively converging to declared state.
    Converging,
    /// Emergency mode — extended window, no auto-rollback.
    Emergency,
}

/// Type of configuration entry in the journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryType {
    Commit,
    Rollback,
    AutoConverge,
    DriftDetected,
    CapabilityChange,
    PolicyUpdate,
    BootConfig,
    EmergencyStart,
    EmergencyEnd,
    ExecLog,
    ShellSession,
    ServiceLifecycle,
}

/// Scope of a configuration entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    Global,
    VCluster(VClusterId),
    Node(NodeId),
}

/// Identity of the actor performing an operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Identity {
    pub principal: String,
    pub principal_type: PrincipalType,
    pub role: String,
}

/// Type of principal performing an operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrincipalType {
    Human,
    Agent,
    Service,
}

/// An immutable configuration entry in the journal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigEntry {
    pub sequence: EntrySeq,
    pub timestamp: DateTime<Utc>,
    pub entry_type: EntryType,
    pub scope: Scope,
    pub author: Identity,
    pub parent: Option<EntrySeq>,
    pub state_delta: Option<StateDelta>,
    pub policy_ref: Option<String>,
    pub ttl_seconds: Option<u32>,
    pub emergency_reason: Option<String>,
}

/// State delta representing changes in a config entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateDelta {
    pub mounts: Vec<DeltaItem>,
    pub files: Vec<DeltaItem>,
    pub network: Vec<DeltaItem>,
    pub services: Vec<DeltaItem>,
    pub kernel: Vec<DeltaItem>,
    pub packages: Vec<DeltaItem>,
    pub gpu: Vec<DeltaItem>,
}

/// A single change within a delta.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaItem {
    pub action: DeltaAction,
    pub key: String,
    pub value: Option<String>,
    pub previous: Option<String>,
}

/// Type of change in a delta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeltaAction {
    Add,
    Remove,
    Modify,
}

/// Drift vector with magnitude per category.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftVector {
    pub mounts: f64,
    pub files: f64,
    pub network: f64,
    pub services: f64,
    pub kernel: f64,
    pub packages: f64,
    pub gpu: f64,
}

impl DriftVector {
    /// Compute total drift magnitude (weighted L2 norm).
    #[must_use]
    #[allow(clippy::suboptimal_flops)]
    pub fn magnitude(&self, weights: &DriftWeights) -> f64 {
        let sum = weights.mounts * self.mounts * self.mounts
            + weights.files * self.files * self.files
            + weights.network * self.network * self.network
            + weights.services * self.services * self.services
            + weights.kernel * self.kernel * self.kernel
            + weights.packages * self.packages * self.packages
            + weights.gpu * self.gpu * self.gpu;
        sum.sqrt()
    }
}

/// Per-category weights for drift magnitude computation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftWeights {
    pub mounts: f64,
    pub files: f64,
    pub network: f64,
    pub services: f64,
    pub kernel: f64,
    pub packages: f64,
    pub gpu: f64,
}

impl Default for DriftWeights {
    fn default() -> Self {
        Self {
            mounts: 1.0,
            files: 1.0,
            network: 1.0,
            services: 1.0,
            kernel: 2.0,
            packages: 1.0,
            gpu: 2.0,
        }
    }
}

/// Status of a supervised service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServiceState {
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
    Restarting,
}

/// Declaration of a service to be supervised.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceDecl {
    pub name: String,
    pub binary: String,
    pub args: Vec<String>,
    pub restart: RestartPolicy,
    pub restart_delay_seconds: u32,
    pub depends_on: Vec<String>,
    pub order: u32,
    pub cgroup_memory_max: Option<String>,
    pub health_check: Option<HealthCheck>,
}

/// Restart policy for supervised services.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RestartPolicy {
    Always,
    OnFailure,
    Never,
}

/// Health check configuration for a service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheck {
    pub check_type: HealthCheckType,
    pub interval_seconds: u32,
}

/// Type of health check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthCheckType {
    Process,
    Http { url: String },
    Tcp { port: u16 },
}

/// Supervisor backend selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SupervisorBackend {
    Pact,
    Systemd,
}

/// Hardware capability report from a node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityReport {
    pub node_id: NodeId,
    pub timestamp: DateTime<Utc>,
    pub report_id: Uuid,
    pub gpus: Vec<GpuCapability>,
    pub memory_bytes: u64,
    pub config_state: ConfigState,
    pub supervisor_status: SupervisorStatus,
}

/// GPU capability information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuCapability {
    pub index: u32,
    pub model: String,
    pub memory_bytes: u64,
    pub pci_bus_id: String,
}

/// Status of the process supervisor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorStatus {
    pub backend: SupervisorBackend,
    pub services_declared: u32,
    pub services_running: u32,
    pub services_failed: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drift_magnitude_zero_when_no_drift() {
        let drift = DriftVector {
            mounts: 0.0,
            files: 0.0,
            network: 0.0,
            services: 0.0,
            kernel: 0.0,
            packages: 0.0,
            gpu: 0.0,
        };
        let weights = DriftWeights::default();
        assert!((drift.magnitude(&weights) - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn drift_magnitude_respects_weights() {
        let drift = DriftVector {
            mounts: 1.0,
            files: 0.0,
            network: 0.0,
            services: 0.0,
            kernel: 1.0,
            packages: 0.0,
            gpu: 0.0,
        };
        let weights = DriftWeights::default();
        // mounts: 1.0*1.0*1.0 = 1.0, kernel: 2.0*1.0*1.0 = 2.0 → sqrt(3.0)
        let expected = 3.0_f64.sqrt();
        assert!((drift.magnitude(&weights) - expected).abs() < 1e-10);
    }

    #[test]
    fn config_state_serialization_roundtrip() {
        let state = ConfigState::Drifted;
        let json = serde_json::to_string(&state).unwrap();
        let deserialized: ConfigState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, deserialized);
    }

    #[test]
    fn service_decl_deserialize_from_toml() {
        let toml_str = r#"
            name = "chronyd"
            binary = "/usr/sbin/chronyd"
            args = ["-d"]
            restart = "Always"
            restart_delay_seconds = 5
            depends_on = []
            order = 1
        "#;
        let decl: ServiceDecl = toml::from_str(toml_str).unwrap();
        assert_eq!(decl.name, "chronyd");
        assert_eq!(decl.restart, RestartPolicy::Always);
        assert_eq!(decl.order, 1);
    }
}
