//! SPIRE identity provider — obtains SVID from SPIRE agent.
//!
//! Connects to the SPIRE Workload API via unix socket using the `spiffe` crate.
//! Primary identity source when SPIRE is deployed (A-I7).
//!
//! Feature-gated behind `spire` — when disabled, a stub is provided
//! that always reports unavailable.

use hpc_identity::{IdentityError, IdentityProvider, IdentitySource, WorkloadIdentity};

/// SPIRE identity provider.
#[allow(dead_code)] // agent_socket used only with `spire` feature
pub struct SpireProvider {
    /// Path to the SPIRE agent Workload API socket.
    agent_socket: String,
}

impl SpireProvider {
    #[must_use]
    pub fn new(agent_socket: &str) -> Self {
        Self { agent_socket: agent_socket.to_string() }
    }
}

#[cfg(feature = "spire")]
#[async_trait::async_trait]
impl IdentityProvider for SpireProvider {
    async fn get_identity(&self) -> Result<WorkloadIdentity, IdentityError> {
        info!(socket = %self.agent_socket, "requesting X.509 SVID from SPIRE agent");

        use spiffe::workload_api::x509::X509Source;

        // Connect to SPIRE Workload API
        let source =
            X509Source::builder().with_socket_path(&self.agent_socket).build().await.map_err(
                |e| IdentityError::SpireUnavailable {
                    reason: format!("failed to connect to SPIRE agent: {e}"),
                },
            )?;

        // Get the default SVID
        let svid = source.svid().map_err(|e| IdentityError::SpireUnavailable {
            reason: format!("failed to get SVID: {e}"),
        })?;

        // Extract cert chain, private key, and trust bundle as PEM
        let cert_chain_pem = svid.cert_chain_pem().map_err(|e| {
            IdentityError::SpireUnavailable { reason: format!("failed to encode cert chain: {e}") }
        })?;
        let private_key_pem = svid.private_key_pem().map_err(|e| {
            IdentityError::SpireUnavailable { reason: format!("failed to encode private key: {e}") }
        })?;

        // Get trust bundle
        let bundle = source.bundle().map_err(|e| IdentityError::SpireUnavailable {
            reason: format!("failed to get trust bundle: {e}"),
        })?;
        let trust_bundle_pem =
            bundle.authorities_pem().map_err(|e| IdentityError::SpireUnavailable {
                reason: format!("failed to encode trust bundle: {e}"),
            })?;

        // Parse expiry from the leaf cert
        let expires_at = svid
            .x509_svid()
            .not_after()
            .and_then(|t| chrono::DateTime::from_timestamp(t.unix_timestamp(), 0))
            .unwrap_or_else(|| chrono::Utc::now() + chrono::Duration::hours(1));

        info!(
            spiffe_id = %svid.spiffe_id(),
            expires_at = %expires_at,
            "SPIRE SVID acquired"
        );

        Ok(WorkloadIdentity {
            cert_chain_pem: cert_chain_pem.into_bytes(),
            private_key_pem: private_key_pem.into_bytes(),
            trust_bundle_pem: trust_bundle_pem.into_bytes(),
            expires_at,
            source: IdentitySource::Spire,
        })
    }

    async fn is_available(&self) -> bool {
        let available = std::path::Path::new(&self.agent_socket).exists();
        debug!(
            socket = %self.agent_socket,
            available = available,
            "SPIRE agent socket check"
        );
        available
    }

    fn source_type(&self) -> IdentitySource {
        IdentitySource::Spire
    }
}

#[cfg(not(feature = "spire"))]
#[async_trait::async_trait]
impl IdentityProvider for SpireProvider {
    async fn get_identity(&self) -> Result<WorkloadIdentity, IdentityError> {
        Err(IdentityError::SpireUnavailable {
            reason: "SPIRE support not compiled in (enable 'spire' feature)".to_string(),
        })
    }

    async fn is_available(&self) -> bool {
        false
    }

    fn source_type(&self) -> IdentitySource {
        IdentitySource::Spire
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn spire_not_available_no_socket() {
        let provider = SpireProvider::new("/nonexistent/spire.sock");
        assert!(!provider.is_available().await);
    }

    #[tokio::test]
    async fn spire_get_identity_returns_unavailable() {
        let provider = SpireProvider::new("/nonexistent/spire.sock");
        let err = provider.get_identity().await.unwrap_err();
        assert!(matches!(err, IdentityError::SpireUnavailable { .. }));
    }

    #[test]
    fn spire_source_type() {
        let provider = SpireProvider::new("/tmp/spire.sock");
        assert_eq!(provider.source_type(), IdentitySource::Spire);
    }
}
