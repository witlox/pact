//! Journal gRPC client — connects the agent to the journal quorum.
//!
//! Wraps tonic clients for ConfigService, BootConfigService, and PolicyService.
//! Handles endpoint selection and reconnection.

use std::sync::Arc;

use tokio::sync::RwLock;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Identity};
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
        let tls = if config.tls_enabled {
            Some(load_tls_config(config)?)
        } else {
            None
        };
        let channel = Self::connect_channel(&config.endpoints, tls.as_ref()).await?;
        Ok(Self {
            config: ConfigServiceClient::new(channel.clone()),
            boot: BootConfigServiceClient::new(channel.clone()),
            policy: PolicyServiceClient::new(channel),
        })
    }

    /// Try endpoints in order, return first successful channel.
    async fn connect_channel(
        endpoints: &[String],
        tls: Option<&ClientTlsConfig>,
    ) -> anyhow::Result<Channel> {
        for endpoint in endpoints {
            let uri = if endpoint.starts_with("http") {
                endpoint.clone()
            } else if tls.is_some() {
                format!("https://{endpoint}")
            } else {
                format!("http://{endpoint}")
            };
            debug!(endpoint = %uri, tls = tls.is_some(), "Trying journal endpoint");
            let mut channel_builder = Channel::from_shared(uri.clone())
                .map_err(|e| anyhow::anyhow!("invalid endpoint {uri}: {e}"))?;

            if let Some(tls_config) = tls {
                channel_builder = channel_builder
                    .tls_config(tls_config.clone())
                    .map_err(|e| anyhow::anyhow!("TLS config error for {uri}: {e}"))?;
            }

            match channel_builder.connect().await {
                Ok(channel) => {
                    info!(endpoint = %uri, tls = tls.is_some(), "Connected to journal");
                    return Ok(channel);
                }
                Err(e) => {
                    warn!(endpoint = %uri, error = %e, "Journal endpoint unreachable");
                }
            }
        }
        Err(anyhow::anyhow!("no reachable journal endpoint (tried {})", endpoints.join(", ")))
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

/// Load mTLS configuration from `JournalConnectionConfig` paths.
///
/// Reads the CA certificate, client certificate, and client key from the paths
/// specified in the config. All three must be present when TLS is enabled.
pub fn load_tls_config(config: &JournalConnectionConfig) -> anyhow::Result<ClientTlsConfig> {
    let ca_path = config
        .tls_ca
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("tls_enabled=true but tls_ca path is not set"))?;
    let cert_path = config
        .tls_cert
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("tls_enabled=true but tls_cert path is not set"))?;
    let key_path = config
        .tls_key
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("tls_enabled=true but tls_key path is not set"))?;

    let ca_pem = std::fs::read_to_string(ca_path)
        .map_err(|e| anyhow::anyhow!("cannot read CA cert {}: {e}", ca_path.display()))?;
    let cert_pem = std::fs::read_to_string(cert_path)
        .map_err(|e| anyhow::anyhow!("cannot read client cert {}: {e}", cert_path.display()))?;
    let key_pem = std::fs::read_to_string(key_path)
        .map_err(|e| anyhow::anyhow!("cannot read client key {}: {e}", key_path.display()))?;

    let tls = ClientTlsConfig::new()
        .ca_certificate(Certificate::from_pem(ca_pem))
        .identity(Identity::from_pem(cert_pem, key_pem));

    Ok(tls)
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

    #[test]
    fn load_tls_config_missing_ca_path() {
        let config = JournalConnectionConfig {
            endpoints: vec![],
            tls_enabled: true,
            tls_cert: Some("/tmp/cert.pem".into()),
            tls_key: Some("/tmp/key.pem".into()),
            tls_ca: None,
        };
        let result = load_tls_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("tls_ca"));
    }

    #[test]
    fn load_tls_config_missing_cert_path() {
        let config = JournalConnectionConfig {
            endpoints: vec![],
            tls_enabled: true,
            tls_cert: None,
            tls_key: Some("/tmp/key.pem".into()),
            tls_ca: Some("/tmp/ca.pem".into()),
        };
        let result = load_tls_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("tls_cert"));
    }

    #[test]
    fn load_tls_config_missing_key_path() {
        let config = JournalConnectionConfig {
            endpoints: vec![],
            tls_enabled: true,
            tls_cert: Some("/tmp/cert.pem".into()),
            tls_key: None,
            tls_ca: Some("/tmp/ca.pem".into()),
        };
        let result = load_tls_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("tls_key"));
    }

    #[test]
    fn load_tls_config_nonexistent_files() {
        let config = JournalConnectionConfig {
            endpoints: vec![],
            tls_enabled: true,
            tls_cert: Some("/nonexistent/cert.pem".into()),
            tls_key: Some("/nonexistent/key.pem".into()),
            tls_ca: Some("/nonexistent/ca.pem".into()),
        };
        let result = load_tls_config(&config);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot read"));
    }
}
