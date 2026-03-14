//! Config subscription — live updates from journal after boot.
//!
//! Subscribes to `BootConfigService.SubscribeConfigUpdates()` and processes
//! incoming `ConfigUpdate` events:
//! - `vcluster_change` → update cached overlay
//! - `node_change` → re-apply node delta
//! - `policy_change` → update cached VClusterPolicy
//! - `blacklist_change` → update observer blacklist
//!
//! Reconnects with `from_sequence` on interruption (at-least-once delivery).

use std::sync::Arc;

use tokio::sync::RwLock;
use tokio_stream::StreamExt;
use tracing::{debug, error, info, warn};

use pact_common::proto::stream::boot_config_service_client::BootConfigServiceClient;
use pact_common::proto::stream::{config_update, SubscribeRequest};
use pact_common::types::VClusterPolicy;
use tonic::transport::Channel;

/// Events emitted by the subscription for the agent to act on.
#[derive(Debug, Clone)]
pub enum ConfigUpdateAction {
    /// vCluster overlay changed — re-apply.
    OverlayChanged { data: Vec<u8> },
    /// Node-specific delta changed — re-apply.
    NodeDeltaChanged { data: Vec<u8> },
    /// Policy updated — replace cached policy.
    PolicyChanged { policy: VClusterPolicy },
    /// Blacklist patterns changed — update observer.
    BlacklistChanged { patterns: Vec<String> },
}

/// Tracks subscription state for reconnection.
#[derive(Debug, Default)]
pub struct SubscriptionState {
    /// Last successfully processed sequence number.
    pub last_sequence: u64,
    /// Number of consecutive reconnect attempts.
    pub reconnect_attempts: u32,
    /// Whether the subscription is currently connected.
    pub connected: bool,
}

/// Configuration for subscription behavior.
#[derive(Debug, Clone)]
pub struct SubscriptionConfig {
    /// Node ID for this agent.
    pub node_id: String,
    /// vCluster this agent belongs to.
    pub vcluster_id: String,
    /// Base reconnect delay in milliseconds.
    pub reconnect_base_ms: u64,
    /// Maximum reconnect delay in milliseconds.
    pub reconnect_max_ms: u64,
    /// Maximum consecutive reconnect attempts before alerting.
    pub max_reconnect_attempts: u32,
}

impl Default for SubscriptionConfig {
    fn default() -> Self {
        Self {
            node_id: String::new(),
            vcluster_id: String::new(),
            reconnect_base_ms: 1000,
            reconnect_max_ms: 60_000,
            max_reconnect_attempts: 100,
        }
    }
}

/// Manages the config subscription lifecycle.
///
/// Processes `ConfigUpdate` messages from journal's `BootConfigService` and
/// dispatches `ConfigUpdateAction` events for the agent to act on.
pub struct ConfigSubscription {
    config: SubscriptionConfig,
    state: Arc<RwLock<SubscriptionState>>,
    /// Channel to send actions to the agent main loop.
    action_tx: tokio::sync::mpsc::Sender<ConfigUpdateAction>,
}

impl ConfigSubscription {
    pub fn new(
        config: SubscriptionConfig,
        action_tx: tokio::sync::mpsc::Sender<ConfigUpdateAction>,
    ) -> Self {
        Self { config, state: Arc::new(RwLock::new(SubscriptionState::default())), action_tx }
    }

    /// Get the current subscription state.
    pub async fn state(&self) -> SubscriptionState {
        let s = self.state.read().await;
        SubscriptionState {
            last_sequence: s.last_sequence,
            reconnect_attempts: s.reconnect_attempts,
            connected: s.connected,
        }
    }

    /// Process a single ConfigUpdate from the stream.
    ///
    /// Deserializes the update payload, dispatches the appropriate action,
    /// and advances the sequence counter.
    pub async fn process_update(&self, sequence: u64, update: UpdatePayload) -> anyhow::Result<()> {
        let action = match update {
            UpdatePayload::VClusterChange(data) => {
                debug!(sequence, "Processing vCluster overlay change");
                ConfigUpdateAction::OverlayChanged { data }
            }
            UpdatePayload::NodeChange(data) => {
                debug!(sequence, "Processing node delta change");
                ConfigUpdateAction::NodeDeltaChanged { data }
            }
            UpdatePayload::PolicyChange(data) => {
                debug!(sequence, "Processing policy change");
                let policy: VClusterPolicy = serde_json::from_slice(&data)
                    .map_err(|e| anyhow::anyhow!("invalid policy payload: {e}"))?;
                ConfigUpdateAction::PolicyChanged { policy }
            }
            UpdatePayload::BlacklistChange(data) => {
                debug!(sequence, "Processing blacklist change");
                let patterns: Vec<String> = serde_json::from_slice(&data)
                    .map_err(|e| anyhow::anyhow!("invalid blacklist payload: {e}"))?;
                ConfigUpdateAction::BlacklistChanged { patterns }
            }
        };

        self.action_tx
            .send(action)
            .await
            .map_err(|e| anyhow::anyhow!("action channel closed: {e}"))?;

        // Advance sequence
        let mut state = self.state.write().await;
        state.last_sequence = sequence;
        Ok(())
    }

    /// Record successful connection.
    pub async fn on_connected(&self) {
        let mut state = self.state.write().await;
        state.connected = true;
        state.reconnect_attempts = 0;
        info!(
            node_id = %self.config.node_id,
            vcluster = %self.config.vcluster_id,
            from_sequence = state.last_sequence,
            "Config subscription connected"
        );
    }

    /// Record disconnection and compute reconnect delay.
    ///
    /// Returns the delay in milliseconds before reconnecting, or `None` if
    /// max attempts exceeded.
    pub async fn on_disconnected(&self) -> Option<u64> {
        let mut state = self.state.write().await;
        state.connected = false;
        state.reconnect_attempts += 1;

        if state.reconnect_attempts > self.config.max_reconnect_attempts {
            error!(
                attempts = state.reconnect_attempts,
                "Max reconnect attempts exceeded — subscription abandoned"
            );
            return None;
        }

        // Exponential backoff with jitter
        let exp = state.reconnect_attempts.min(10);
        let delay = self.config.reconnect_base_ms * 2u64.pow(exp);
        let delay = delay.min(self.config.reconnect_max_ms);

        warn!(
            attempt = state.reconnect_attempts,
            delay_ms = delay,
            "Config subscription disconnected, will reconnect"
        );

        Some(delay)
    }

    /// Get the sequence number to use for reconnection.
    pub async fn from_sequence(&self) -> u64 {
        self.state.read().await.last_sequence
    }

    /// Get the subscription config.
    pub fn config(&self) -> &SubscriptionConfig {
        &self.config
    }

    /// Run the subscription loop — connects to journal and processes updates.
    ///
    /// This is the main entry point, meant to be spawned as a tokio task.
    /// It connects, processes the stream, and reconnects with backoff on failure.
    /// Returns only when max reconnect attempts are exceeded or the action channel closes.
    pub async fn run(&self, mut client: BootConfigServiceClient<Channel>) {
        loop {
            let from_seq = self.from_sequence().await;
            let request = tonic::Request::new(SubscribeRequest {
                node_id: self.config.node_id.clone(),
                vcluster_id: self.config.vcluster_id.clone(),
                from_sequence: from_seq,
            });

            match client.subscribe_config_updates(request).await {
                Ok(response) => {
                    self.on_connected().await;
                    let mut stream = response.into_inner();

                    while let Some(result) = stream.next().await {
                        match result {
                            Ok(update) => {
                                let payload = match update.update {
                                    Some(config_update::Update::VclusterChange(data)) => {
                                        UpdatePayload::VClusterChange(data)
                                    }
                                    Some(config_update::Update::NodeChange(data)) => {
                                        UpdatePayload::NodeChange(data)
                                    }
                                    Some(config_update::Update::PolicyChange(data)) => {
                                        UpdatePayload::PolicyChange(data)
                                    }
                                    Some(config_update::Update::BlacklistChange(data)) => {
                                        UpdatePayload::BlacklistChange(data)
                                    }
                                    None => continue,
                                };

                                if let Err(e) =
                                    self.process_update(update.sequence, payload).await
                                {
                                    error!(error = %e, "Failed to process config update");
                                }
                            }
                            Err(e) => {
                                warn!(error = %e, "Config update stream error");
                                break; // reconnect
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Failed to subscribe to config updates");
                }
            }

            // Disconnected — attempt reconnect with backoff
            if let Some(delay_ms) = self.on_disconnected().await {
                tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
            } else {
                error!("Subscription abandoned after max reconnect attempts");
                return;
            }
        }
    }
}

/// Decoded update payload (matches proto oneof).
#[derive(Debug, Clone)]
pub enum UpdatePayload {
    VClusterChange(Vec<u8>),
    NodeChange(Vec<u8>),
    PolicyChange(Vec<u8>),
    BlacklistChange(Vec<u8>),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> SubscriptionConfig {
        SubscriptionConfig {
            node_id: "node-001".into(),
            vcluster_id: "ml-training".into(),
            reconnect_base_ms: 100,
            reconnect_max_ms: 5000,
            max_reconnect_attempts: 5,
        }
    }

    #[tokio::test]
    async fn process_overlay_change() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let sub = ConfigSubscription::new(test_config(), tx);

        sub.process_update(1, UpdatePayload::VClusterChange(vec![1, 2, 3])).await.unwrap();

        let action = rx.recv().await.unwrap();
        assert!(
            matches!(action, ConfigUpdateAction::OverlayChanged { data } if data == vec![1, 2, 3])
        );

        let state = sub.state().await;
        assert_eq!(state.last_sequence, 1);
    }

    #[tokio::test]
    async fn process_node_delta_change() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let sub = ConfigSubscription::new(test_config(), tx);

        sub.process_update(5, UpdatePayload::NodeChange(vec![10, 20])).await.unwrap();

        let action = rx.recv().await.unwrap();
        assert!(
            matches!(action, ConfigUpdateAction::NodeDeltaChanged { data } if data == vec![10, 20])
        );
        assert_eq!(sub.from_sequence().await, 5);
    }

    #[tokio::test]
    async fn process_policy_change() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let sub = ConfigSubscription::new(test_config(), tx);

        let policy = VClusterPolicy::default();
        let payload = serde_json::to_vec(&policy).unwrap();

        sub.process_update(10, UpdatePayload::PolicyChange(payload)).await.unwrap();

        let action = rx.recv().await.unwrap();
        assert!(matches!(action, ConfigUpdateAction::PolicyChanged { .. }));
    }

    #[tokio::test]
    async fn process_blacklist_change() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let sub = ConfigSubscription::new(test_config(), tx);

        let patterns = vec!["/tmp/**".to_string(), "/var/log/**".to_string()];
        let payload = serde_json::to_vec(&patterns).unwrap();

        sub.process_update(15, UpdatePayload::BlacklistChange(payload)).await.unwrap();

        let action = rx.recv().await.unwrap();
        match action {
            ConfigUpdateAction::BlacklistChanged { patterns: p } => {
                assert_eq!(p, vec!["/tmp/**", "/var/log/**"]);
            }
            other => panic!("expected BlacklistChanged, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn invalid_policy_payload_returns_error() {
        let (tx, _rx) = tokio::sync::mpsc::channel(10);
        let sub = ConfigSubscription::new(test_config(), tx);

        let result = sub.process_update(1, UpdatePayload::PolicyChange(vec![0xFF, 0xFE])).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn reconnect_backoff_exponential() {
        let (tx, _rx) = tokio::sync::mpsc::channel(10);
        let sub = ConfigSubscription::new(test_config(), tx);

        // First disconnect: delay = 100 * 2^1 = 200
        let delay1 = sub.on_disconnected().await.unwrap();
        assert_eq!(delay1, 200);

        // Second: 100 * 2^2 = 400
        let delay2 = sub.on_disconnected().await.unwrap();
        assert_eq!(delay2, 400);

        // Third: 100 * 2^3 = 800
        let delay3 = sub.on_disconnected().await.unwrap();
        assert_eq!(delay3, 800);
    }

    #[tokio::test]
    async fn reconnect_delay_capped_at_max() {
        let (tx, _rx) = tokio::sync::mpsc::channel(10);
        let config = SubscriptionConfig {
            reconnect_base_ms: 1000,
            reconnect_max_ms: 5000,
            max_reconnect_attempts: 20,
            ..test_config()
        };
        let sub = ConfigSubscription::new(config, tx);

        // Collect all delays and verify they never exceed max
        let mut delays = Vec::new();
        for _ in 0..15 {
            let delay = sub.on_disconnected().await.unwrap();
            delays.push(delay);
        }

        // All delays should be <= 5000
        for (i, delay) in delays.iter().enumerate() {
            assert!(*delay <= 5000, "delay at attempt {} was {} (should be <= 5000)", i + 1, delay);
        }

        // Later delays should be capped at exactly 5000
        // At attempt 10+: 1000 * 2^10 = 1024000, capped to 5000
        assert_eq!(*delays.last().unwrap(), 5000);

        // Earlier delays should be smaller (exponential growth)
        assert!(delays[0] < delays[2], "delays should grow exponentially");
    }

    #[tokio::test]
    async fn max_reconnect_attempts_returns_none() {
        let (tx, _rx) = tokio::sync::mpsc::channel(10);
        let sub = ConfigSubscription::new(test_config(), tx);

        // Exhaust max_reconnect_attempts (5)
        for _ in 0..5 {
            assert!(sub.on_disconnected().await.is_some());
        }
        // 6th attempt should return None
        assert!(sub.on_disconnected().await.is_none());
    }

    #[tokio::test]
    async fn on_connected_resets_attempts() {
        let (tx, _rx) = tokio::sync::mpsc::channel(10);
        let sub = ConfigSubscription::new(test_config(), tx);

        sub.on_disconnected().await;
        sub.on_disconnected().await;
        assert_eq!(sub.state().await.reconnect_attempts, 2);

        sub.on_connected().await;
        let state = sub.state().await;
        assert_eq!(state.reconnect_attempts, 0);
        assert!(state.connected);
    }

    #[tokio::test]
    async fn sequence_advances_through_updates() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(10);
        let sub = ConfigSubscription::new(test_config(), tx);

        assert_eq!(sub.from_sequence().await, 0);

        sub.process_update(1, UpdatePayload::VClusterChange(vec![])).await.unwrap();
        assert_eq!(sub.from_sequence().await, 1);

        sub.process_update(5, UpdatePayload::NodeChange(vec![])).await.unwrap();
        assert_eq!(sub.from_sequence().await, 5);

        // Drain the channel
        rx.recv().await;
        rx.recv().await;
    }
}
