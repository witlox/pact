//! Journal gRPC client — connects the agent to the journal quorum.
//!
//! Wraps tonic clients for ConfigService, BootConfigService, and PolicyService.
//! Handles endpoint selection and reconnection.

use std::sync::Arc;

use tokio::sync::RwLock;
use tonic::transport::Channel;
use tracing::{debug, error, info, warn};

use pact_common::config::JournalConnectionConfig;
use pact_common::proto::journal::config_service_client::ConfigServiceClient;
use pact_common::proto::policy::policy_service_client::PolicyServiceClient;
use pact_common::proto::stream::boot_config_service_client::BootConfigServiceClient;

/// Client for communicating with the journal quorum.
///
/// Tries each endpoint in order until one connects. Reconnects transparently
/// on channel failures.
#[derive(Debug, Clone)]
pub struct JournalClient {
    config: ConfigServiceClient<Channel>,
    boot: BootConfigServiceClient<Channel>,
    policy: PolicyServiceClient<Channel>,
}

impl JournalClient {
    /// Connect to the first reachable journal endpoint.
    pub async fn connect(config: &JournalConnectionConfig) -> anyhow::Result<Self> {
        let channel = Self::connect_channel(&config.endpoints).await?;
        Ok(Self {
            config: ConfigServiceClient::new(channel.clone()),
            boot: BootConfigServiceClient::new(channel.clone()),
            policy: PolicyServiceClient::new(channel),
        })
    }

    /// Try endpoints in order, return first successful channel.
    async fn connect_channel(endpoints: &[String]) -> anyhow::Result<Channel> {
        for endpoint in endpoints {
            let uri = if endpoint.starts_with("http") {
                endpoint.clone()
            } else {
                format!("http://{endpoint}")
            };
            debug!(endpoint = %uri, "Trying journal endpoint");
            match Channel::from_shared(uri.clone())
                .map_err(|e| anyhow::anyhow!("invalid endpoint {uri}: {e}"))?
                .connect()
                .await
            {
                Ok(channel) => {
                    info!(endpoint = %uri, "Connected to journal");
                    return Ok(channel);
                }
                Err(e) => {
                    warn!(endpoint = %uri, error = %e, "Journal endpoint unreachable");
                }
            }
        }
        Err(anyhow::anyhow!(
            "no reachable journal endpoint (tried {})",
            endpoints.join(", ")
        ))
    }

    /// Get the ConfigService client.
    pub fn config_service(&self) -> ConfigServiceClient<Channel> {
        self.config.clone()
    }

    /// Get the BootConfigService client.
    pub fn boot_config(&self) -> BootConfigServiceClient<Channel> {
        self.boot.clone()
    }

    /// Get the PolicyService client.
    pub fn policy_service(&self) -> PolicyServiceClient<Channel> {
        self.policy.clone()
    }
}

/// Optional journal client — `None` when running without a journal (dev mode).
pub type OptionalJournalClient = Arc<RwLock<Option<JournalClient>>>;

/// Create a journal client, logging a warning if connection fails.
///
/// Returns `None` instead of failing — the agent should be able to start
/// without journal connectivity and reconnect later.
pub async fn try_connect(config: &JournalConnectionConfig) -> Option<JournalClient> {
    match JournalClient::connect(config).await {
        Ok(client) => {
            info!("Journal client connected");
            Some(client)
        }
        Err(e) => {
            error!(error = %e, "Failed to connect to journal — running in disconnected mode");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn connect_fails_gracefully_with_no_endpoints() {
        let config = JournalConnectionConfig {
            endpoints: vec![],
            tls_enabled: false,
            tls_cert: None,
            tls_key: None,
            tls_ca: None,
        };
        let result = JournalClient::connect(&config).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no reachable"));
    }

    #[tokio::test]
    async fn connect_fails_gracefully_with_unreachable_endpoint() {
        let config = JournalConnectionConfig {
            endpoints: vec!["http://127.0.0.1:19999".into()],
            tls_enabled: false,
            tls_cert: None,
            tls_key: None,
            tls_ca: None,
        };
        let result = JournalClient::connect(&config).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn try_connect_returns_none_on_failure() {
        let config = JournalConnectionConfig {
            endpoints: vec!["http://127.0.0.1:19999".into()],
            tls_enabled: false,
            tls_cert: None,
            tls_key: None,
            tls_ca: None,
        };
        let client = try_connect(&config).await;
        assert!(client.is_none());
    }
}
