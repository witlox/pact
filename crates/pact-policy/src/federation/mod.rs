//! Sovra federation — policy template synchronization.
//!
//! Feature-gated behind `federation`. When enabled:
//! - Syncs Rego policy templates from Sovra on configurable interval
//! - Templates stored locally in `/etc/pact/policies/`
//! - Loaded into OPA as bundles
//! - Site-local data (config, audit, drift) NEVER leaves site
//!
//! When disabled or Sovra unreachable: graceful degradation using cached templates.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tracing::{info, warn};

/// Trait for federation synchronization.
#[async_trait]
pub trait FederationSync: Send + Sync {
    /// Sync policy templates from Sovra.
    async fn sync(&self) -> Result<SyncResult, FederationError>;

    /// Check if Sovra is reachable.
    async fn health(&self) -> bool;
}

/// Result of a federation sync.
#[derive(Debug, Clone)]
pub struct SyncResult {
    /// Number of templates updated.
    pub templates_updated: u32,
    /// Total templates available.
    pub templates_total: u32,
    /// Timestamp of sync.
    pub synced_at: DateTime<Utc>,
}

/// Federation sync configuration.
#[derive(Debug, Clone)]
pub struct FederationConfig {
    /// Sovra endpoint URL.
    pub sovra_endpoint: String,
    /// Sync interval in seconds (default 300).
    pub sync_interval_seconds: u32,
    /// Local directory for policy templates.
    pub templates_dir: String,
}

impl Default for FederationConfig {
    fn default() -> Self {
        Self {
            sovra_endpoint: String::new(),
            sync_interval_seconds: 300,
            templates_dir: "/etc/pact/policies".into(),
        }
    }
}

/// Federation state tracking.
#[derive(Debug, Clone)]
pub struct FederationState {
    /// Last successful sync timestamp.
    pub last_sync: Option<DateTime<Utc>>,
    /// Whether Sovra is currently reachable.
    pub connected: bool,
    /// Number of consecutive sync failures.
    pub failure_count: u32,
    /// Cached template names.
    pub templates: Vec<String>,
}

impl Default for FederationState {
    fn default() -> Self {
        Self { last_sync: None, connected: false, failure_count: 0, templates: Vec::new() }
    }
}

impl FederationState {
    /// Record a successful sync.
    pub fn on_sync_success(&mut self, result: &SyncResult) {
        self.last_sync = Some(result.synced_at);
        self.connected = true;
        self.failure_count = 0;
        info!(
            templates_updated = result.templates_updated,
            templates_total = result.templates_total,
            "Federation sync completed"
        );
    }

    /// Record a sync failure (F10: graceful degradation).
    pub fn on_sync_failure(&mut self, error: &FederationError) {
        self.connected = false;
        self.failure_count += 1;
        warn!(
            failure_count = self.failure_count,
            error = %error,
            "Federation sync failed — using cached templates (F10)"
        );
    }

    /// Check if sync is overdue.
    pub fn is_sync_overdue(&self, interval_seconds: u32) -> bool {
        match self.last_sync {
            None => true,
            Some(last) => {
                let elapsed = (Utc::now() - last).num_seconds();
                elapsed > i64::from(interval_seconds)
            }
        }
    }
}

/// Mock federation sync for development/testing.
pub struct MockFederationSync {
    /// Whether to simulate successful syncs.
    pub healthy: bool,
    /// Templates to return.
    pub templates: Vec<String>,
}

impl MockFederationSync {
    pub fn healthy(templates: Vec<String>) -> Self {
        Self { healthy: true, templates }
    }

    pub fn unhealthy() -> Self {
        Self { healthy: false, templates: vec![] }
    }
}

#[async_trait]
impl FederationSync for MockFederationSync {
    async fn sync(&self) -> Result<SyncResult, FederationError> {
        if self.healthy {
            Ok(SyncResult {
                templates_updated: self.templates.len() as u32,
                templates_total: self.templates.len() as u32,
                synced_at: Utc::now(),
            })
        } else {
            Err(FederationError::Unreachable("mock: Sovra unavailable".into()))
        }
    }

    async fn health(&self) -> bool {
        self.healthy
    }
}

/// Federation errors.
#[derive(Debug, Clone, thiserror::Error)]
pub enum FederationError {
    #[error("Sovra endpoint unreachable: {0}")]
    Unreachable(String),
    #[error("template sync failed: {0}")]
    SyncFailed(String),
    #[error("invalid template: {0}")]
    InvalidTemplate(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn federation_state_sync_success() {
        let mut state = FederationState::default();
        assert!(state.last_sync.is_none());
        assert!(!state.connected);

        let result = SyncResult { templates_updated: 3, templates_total: 5, synced_at: Utc::now() };
        state.on_sync_success(&result);

        assert!(state.last_sync.is_some());
        assert!(state.connected);
        assert_eq!(state.failure_count, 0);
    }

    #[test]
    fn federation_state_sync_failure_increments() {
        let mut state = FederationState::default();
        let err = FederationError::Unreachable("timeout".into());

        state.on_sync_failure(&err);
        assert_eq!(state.failure_count, 1);
        assert!(!state.connected);

        state.on_sync_failure(&err);
        assert_eq!(state.failure_count, 2);
    }

    #[test]
    fn federation_state_success_resets_failures() {
        let mut state = FederationState::default();
        let err = FederationError::Unreachable("timeout".into());
        state.on_sync_failure(&err);
        state.on_sync_failure(&err);
        assert_eq!(state.failure_count, 2);

        let result = SyncResult { templates_updated: 1, templates_total: 1, synced_at: Utc::now() };
        state.on_sync_success(&result);
        assert_eq!(state.failure_count, 0);
        assert!(state.connected);
    }

    #[test]
    fn sync_overdue_when_never_synced() {
        let state = FederationState::default();
        assert!(state.is_sync_overdue(300));
    }

    #[test]
    fn sync_overdue_when_interval_passed() {
        let mut state = FederationState::default();
        state.last_sync = Some(Utc::now() - chrono::Duration::seconds(600));
        assert!(state.is_sync_overdue(300)); // 600s > 300s interval
    }

    #[test]
    fn sync_not_overdue_when_recent() {
        let mut state = FederationState::default();
        state.last_sync = Some(Utc::now());
        assert!(!state.is_sync_overdue(300));
    }

    #[tokio::test]
    async fn mock_healthy_sync_succeeds() {
        let sync = MockFederationSync::healthy(vec!["exec.rego".into(), "commit.rego".into()]);
        let result = sync.sync().await.unwrap();
        assert_eq!(result.templates_updated, 2);
        assert_eq!(result.templates_total, 2);
    }

    #[tokio::test]
    async fn mock_unhealthy_sync_fails() {
        let sync = MockFederationSync::unhealthy();
        let result = sync.sync().await;
        assert!(matches!(result, Err(FederationError::Unreachable(_))));
    }

    #[tokio::test]
    async fn mock_health_check() {
        assert!(MockFederationSync::healthy(vec![]).health().await);
        assert!(!MockFederationSync::unhealthy().health().await);
    }

    #[test]
    fn federation_config_defaults() {
        let config = FederationConfig::default();
        assert_eq!(config.sync_interval_seconds, 300);
        assert_eq!(config.templates_dir, "/etc/pact/policies");
    }
}
