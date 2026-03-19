//! Configuration structures for pact components.
//!
//! Config format is TOML. Each component has its own config section.
//! Optional sections are `Option<T>` with `#[serde(default)]`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::types::SupervisorBackend;

/// Top-level pact configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PactConfig {
    #[serde(default)]
    pub agent: Option<AgentConfig>,
    #[serde(default)]
    pub journal: Option<JournalConfig>,
    #[serde(default)]
    pub policy: Option<PolicyConfig>,
    #[serde(default)]
    pub telemetry: TelemetryConfig,
}

/// Agent configuration (per-node daemon).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub node_id: String,
    /// vCluster — now optional (set via enrollment or maintained in maintenance mode).
    #[serde(default)]
    pub vcluster: Option<String>,
    #[serde(default = "default_enforcement_mode")]
    pub enforcement_mode: String,
    #[serde(default)]
    pub supervisor: SupervisorConfig,
    pub journal: JournalConnectionConfig,
    #[serde(default)]
    pub observer: ObserverConfig,
    #[serde(default)]
    pub shell: ShellConfig,
    #[serde(default)]
    pub commit_window: CommitWindowConfig,
    #[serde(default)]
    pub blacklist: BlacklistConfig,
    #[serde(default)]
    pub capability: Option<CapabilityConfig>,
    /// Enrollment configuration (enables enrollment workflow if present).
    #[serde(default)]
    pub enrollment: Option<EnrollmentConfig>,
}

/// Agent-side enrollment configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollmentConfig {
    /// Journal endpoints for enrollment (server-TLS-only, no client cert).
    pub journal_endpoints: Vec<String>,
    /// Path to CA certificate for validating the journal's server cert.
    pub ca_cert: PathBuf,
    /// Directory to store the agent's keypair and signed certificate.
    #[serde(default = "default_cert_dir")]
    pub cert_dir: PathBuf,
    /// Certificate renewal interval in seconds (default: 12 hours before expiry).
    #[serde(default = "default_renewal_before_expiry")]
    pub renewal_before_expiry_seconds: u32,
}

fn default_cert_dir() -> PathBuf {
    PathBuf::from("/var/lib/pact/certs")
}

const fn default_renewal_before_expiry() -> u32 {
    43200 // 12 hours
}

fn default_enforcement_mode() -> String {
    "observe".to_string()
}

/// Supervisor backend configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupervisorConfig {
    #[serde(default = "default_supervisor_backend")]
    pub backend: SupervisorBackend,
}

const fn default_supervisor_backend() -> SupervisorBackend {
    SupervisorBackend::Pact
}

impl Default for SupervisorConfig {
    fn default() -> Self {
        Self { backend: default_supervisor_backend() }
    }
}

/// Connection config for agent → journal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalConnectionConfig {
    pub endpoints: Vec<String>,
    #[serde(default)]
    pub tls_enabled: bool,
    pub tls_cert: Option<PathBuf>,
    pub tls_key: Option<PathBuf>,
    pub tls_ca: Option<PathBuf>,
}

/// Observer subsystem configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObserverConfig {
    #[serde(default)]
    pub ebpf_enabled: bool,
    #[serde(default = "default_true")]
    pub inotify_enabled: bool,
    #[serde(default = "default_true")]
    pub netlink_enabled: bool,
}

const fn default_true() -> bool {
    true
}

impl Default for ObserverConfig {
    fn default() -> Self {
        Self { ebpf_enabled: false, inotify_enabled: true, netlink_enabled: true }
    }
}

/// Shell server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_shell_listen")]
    pub listen: String,
    #[serde(default = "default_whitelist_mode")]
    pub whitelist_mode: String,
    #[serde(default)]
    pub auth: Option<AuthAgentConfig>,
}

/// Auth configuration for the shell server.
///
/// Fail-closed: no defaults for issuer/audience/secrets. If this section is
/// missing, the agent starts without auth capability (no HMAC secret, no JWKS).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthAgentConfig {
    /// OIDC issuer URL (e.g., "https://auth.example.com/realms/hpc").
    pub issuer: String,
    /// Expected JWT audience (e.g., "pact-agent").
    pub audience: String,
    /// HMAC secret for development/testing (Base64-encoded). Not for production.
    #[serde(default)]
    pub hmac_secret: Option<String>,
    /// JWKS endpoint URL for RS256 validation. Derived from issuer if not set.
    #[serde(default)]
    pub jwks_url: Option<String>,
}

fn default_shell_listen() -> String {
    "0.0.0.0:9445".to_string()
}

fn default_whitelist_mode() -> String {
    "learning".to_string()
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            listen: default_shell_listen(),
            whitelist_mode: default_whitelist_mode(),
            auth: None,
        }
    }
}

/// Commit window configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitWindowConfig {
    #[serde(default = "default_base_window")]
    pub base_window_seconds: u32,
    #[serde(default = "default_sensitivity")]
    pub drift_sensitivity: f64,
    #[serde(default = "default_emergency_window")]
    pub emergency_window_seconds: u32,
}

const fn default_base_window() -> u32 {
    900
}
const fn default_sensitivity() -> f64 {
    2.0
}
const fn default_emergency_window() -> u32 {
    14400
}

impl Default for CommitWindowConfig {
    fn default() -> Self {
        Self {
            base_window_seconds: default_base_window(),
            drift_sensitivity: default_sensitivity(),
            emergency_window_seconds: default_emergency_window(),
        }
    }
}

/// Blacklist patterns for drift detection exclusion.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlacklistConfig {
    #[serde(default = "default_blacklist_patterns")]
    pub patterns: Vec<String>,
}

fn default_blacklist_patterns() -> Vec<String> {
    vec![
        "/tmp/**".to_string(),
        "/var/log/**".to_string(),
        "/proc/**".to_string(),
        "/sys/**".to_string(),
        "/dev/**".to_string(),
        "/run/user/**".to_string(),
    ]
}

impl Default for BlacklistConfig {
    fn default() -> Self {
        Self { patterns: default_blacklist_patterns() }
    }
}

/// Capability reporter configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityConfig {
    pub manifest_path: PathBuf,
    pub socket_path: PathBuf,
    #[serde(default = "default_gpu_poll_interval")]
    pub gpu_poll_interval_seconds: u32,
}

const fn default_gpu_poll_interval() -> u32 {
    30
}

/// Journal server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalConfig {
    #[serde(default = "default_journal_listen")]
    pub listen_addr: String,
    pub data_dir: PathBuf,
    #[serde(default)]
    pub raft: Option<RaftConfig>,
    #[serde(default)]
    pub streaming: Option<StreamingConfig>,
    /// Enrollment and CA configuration (enables enrollment service if present).
    #[serde(default)]
    pub enrollment: Option<EnrollmentJournalConfig>,
}

/// Journal-side enrollment and CA signing configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrollmentJournalConfig {
    /// Path to intermediate CA certificate (PEM).
    pub ca_cert: PathBuf,
    /// Path to intermediate CA private key (PEM).
    pub ca_key: PathBuf,
    /// Certificate lifetime in seconds (default: 3 days = 259200).
    #[serde(default = "default_cert_lifetime")]
    pub cert_lifetime_seconds: u32,
    /// Enrollment rate limit: max requests per minute.
    #[serde(default = "default_enrollment_rate_limit")]
    pub rate_limit_per_minute: u32,
    /// Heartbeat timeout in seconds (default: 300 = 5 minutes).
    #[serde(default = "default_heartbeat_timeout")]
    pub heartbeat_timeout_seconds: u32,
}

const fn default_cert_lifetime() -> u32 {
    259_200 // 3 days
}

const fn default_enrollment_rate_limit() -> u32 {
    100
}

const fn default_heartbeat_timeout() -> u32 {
    300 // 5 minutes
}

fn default_journal_listen() -> String {
    "0.0.0.0:9443".to_string()
}

/// Raft consensus configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaftConfig {
    pub members: Vec<String>,
    #[serde(default = "default_snapshot_interval")]
    pub snapshot_interval: u64,
}

const fn default_snapshot_interval() -> u64 {
    10000
}

/// Boot config streaming configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingConfig {
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_boot_streams: u32,
}

const fn default_max_concurrent() -> u32 {
    15000
}

/// Policy engine configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub iam: Option<IamConfig>,
    #[serde(default)]
    pub engine: Option<PolicyEngineConfig>,
    #[serde(default)]
    pub federation: Option<FederationConfig>,
}

/// OIDC/SAML identity provider configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IamConfig {
    pub oidc_issuer: String,
    pub oidc_audience: String,
}

/// Policy evaluation engine configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyEngineConfig {
    #[serde(rename = "type")]
    pub engine_type: String,
    pub opa_endpoint: Option<String>,
}

/// Sovra federation configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FederationConfig {
    pub sovra_endpoint: String,
    #[serde(default = "default_sync_interval")]
    pub sync_interval_seconds: u32,
}

const fn default_sync_interval() -> u32 {
    300
}

/// External system delegation endpoints.
///
/// Used by the CLI to connect to lattice (drain/cordon/uncordon) and
/// OpenCHAMI (reboot/reimage) APIs. All fields are optional — delegation
/// commands return "not configured" when the relevant endpoint is absent.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DelegationConfig {
    /// Lattice gRPC endpoint (e.g., "http://localhost:50051").
    pub lattice_endpoint: Option<String>,
    /// Lattice auth token.
    pub lattice_token: Option<String>,
    /// OpenCHAMI SMD base URL (e.g., "https://smd.example.com").
    pub openchami_smd_url: Option<String>,
    /// OpenCHAMI auth token.
    pub openchami_token: Option<String>,
    /// Timeout in seconds for delegation calls.
    pub timeout_secs: u64,
}

impl Default for DelegationConfig {
    fn default() -> Self {
        Self {
            lattice_endpoint: None,
            lattice_token: None,
            openchami_smd_url: None,
            openchami_token: None,
            timeout_secs: 30,
        }
    }
}

/// Telemetry and logging configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_log_format")]
    pub log_format: String,
    #[serde(default)]
    pub prometheus_enabled: bool,
    #[serde(default)]
    pub prometheus_listen: Option<String>,
    #[serde(default)]
    pub loki_enabled: bool,
    #[serde(default)]
    pub loki_endpoint: Option<String>,
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_format() -> String {
    "text".to_string()
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            log_level: default_log_level(),
            log_format: default_log_format(),
            prometheus_enabled: false,
            prometheus_listen: None,
            loki_enabled: false,
            loki_endpoint: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_config_deserializes() {
        let toml_str = r#"
            [agent]
            node_id = "dev-node-001"
            vcluster = "dev-sandbox"

            [agent.journal]
            endpoints = ["localhost:9443"]

            [telemetry]
            log_level = "debug"
        "#;
        let config: PactConfig = toml::from_str(toml_str).unwrap();
        let agent = config.agent.unwrap();
        assert_eq!(agent.node_id, "dev-node-001");
        assert_eq!(agent.vcluster.as_deref(), Some("dev-sandbox"));
        assert_eq!(agent.enforcement_mode, "observe");
        assert_eq!(agent.supervisor.backend, SupervisorBackend::Pact);
    }

    #[test]
    fn config_without_vcluster_deserializes() {
        let toml_str = r#"
            [agent]
            node_id = "dev-node-001"

            [agent.journal]
            endpoints = ["localhost:9443"]

            [telemetry]
            log_level = "debug"
        "#;
        let config: PactConfig = toml::from_str(toml_str).unwrap();
        let agent = config.agent.unwrap();
        assert_eq!(agent.node_id, "dev-node-001");
        assert!(agent.vcluster.is_none());
    }

    #[test]
    fn defaults_are_sensible() {
        let config = TelemetryConfig::default();
        assert_eq!(config.log_level, "info");
        assert_eq!(config.log_format, "text");
        assert!(!config.prometheus_enabled);
    }

    #[test]
    fn commit_window_defaults() {
        let config = CommitWindowConfig::default();
        assert_eq!(config.base_window_seconds, 900);
        assert!((config.drift_sensitivity - 2.0).abs() < f64::EPSILON);
        assert_eq!(config.emergency_window_seconds, 14400);
    }

    #[test]
    fn delegation_config_defaults() {
        let config = DelegationConfig::default();
        assert!(config.lattice_endpoint.is_none());
        assert!(config.lattice_token.is_none());
        assert!(config.openchami_smd_url.is_none());
        assert!(config.openchami_token.is_none());
        assert_eq!(config.timeout_secs, 30);
    }

    #[test]
    fn delegation_config_deserializes() {
        let toml_str = r#"
            lattice_endpoint = "http://lattice:50051"
            openchami_smd_url = "https://smd.example.com"
            timeout_secs = 60
        "#;
        let config: DelegationConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.lattice_endpoint.as_deref(), Some("http://lattice:50051"));
        assert_eq!(config.openchami_smd_url.as_deref(), Some("https://smd.example.com"));
        assert!(config.lattice_token.is_none());
        assert_eq!(config.timeout_secs, 60);
    }

    #[test]
    fn blacklist_default_patterns() {
        let config = BlacklistConfig::default();
        assert!(config.patterns.contains(&"/tmp/**".to_string()));
        assert!(config.patterns.contains(&"/proc/**".to_string()));
        assert!(!config.patterns.is_empty());
    }
}
