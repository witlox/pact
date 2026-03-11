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
    pub vcluster: String,
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
        }
    }
}

/// Commit window configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommitWindowConfig {
    #[serde(default = "default_base_window")]
    pub base_window_seconds: u32,
    #[serde(default = "default_sensitivity")]
    pub sensitivity: f64,
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
            sensitivity: default_sensitivity(),
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
        assert_eq!(agent.enforcement_mode, "observe");
        assert_eq!(agent.supervisor.backend, SupervisorBackend::Pact);
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
        assert!((config.sensitivity - 2.0).abs() < f64::EPSILON);
        assert_eq!(config.emergency_window_seconds, 14400);
    }

    #[test]
    fn blacklist_default_patterns() {
        let config = BlacklistConfig::default();
        assert!(config.patterns.contains(&"/tmp/**".to_string()));
        assert!(config.patterns.contains(&"/proc/**".to_string()));
        assert!(!config.patterns.is_empty());
    }
}
