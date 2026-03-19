//! SPIRE identity provider — obtains SVID from SPIRE agent.
//!
//! Connects to the SPIRE Workload API via unix socket using the `spiffe` crate.
//! Primary identity source when SPIRE is deployed (A-I7).
//!
//! Feature-gated behind `spire` — when disabled, a stub is provided
//! that always reports unavailable.

use hpc_identity::{IdentityError, IdentityProvider, IdentitySource, WorkloadIdentity};

/// Encode DER bytes as PEM with the given label.
#[cfg(feature = "spire")]
fn der_to_pem(der: &[u8], label: &str) -> String {
    use base64::Engine;
    let b64 = base64::engine::general_purpose::STANDARD.encode(der);
    let mut pem = format!("-----BEGIN {label}-----\n");
    for chunk in b64.as_bytes().chunks(64) {
        pem.push_str(std::str::from_utf8(chunk).unwrap_or(""));
        pem.push('\n');
    }
    pem.push_str(&format!("-----END {label}-----"));
    pem
}

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
        use spiffe::bundle::BundleSource;
        use spiffe::X509Source;
        use tracing::{debug, info};

        info!(socket = %self.agent_socket, "requesting X.509 SVID from SPIRE agent");

        // Connect to SPIRE Workload API
        let endpoint = format!("unix://{}", self.agent_socket);
        let source = X509Source::builder().endpoint(&endpoint).build().await.map_err(|e| {
            IdentityError::SpireUnavailable {
                reason: format!("failed to connect to SPIRE agent: {e}"),
            }
        })?;

        // Get the default SVID
        let svid = source.svid().map_err(|e| IdentityError::SpireUnavailable {
            reason: format!("failed to get SVID: {e}"),
        })?;

        // Convert DER cert chain to PEM
        let cert_chain_pem = svid
            .cert_chain()
            .iter()
            .map(|c| der_to_pem(c.as_bytes(), "CERTIFICATE"))
            .collect::<Vec<_>>()
            .join("\n");

        // Convert DER private key to PEM
        let private_key_pem = der_to_pem(svid.private_key().as_bytes(), "PRIVATE KEY");

        // Get trust bundle for the SVID's trust domain
        let td = svid.spiffe_id().trust_domain().clone();
        let bundle = source
            .bundle_for_trust_domain(&td)
            .map_err(|e| IdentityError::SpireUnavailable {
                reason: format!("failed to get trust bundle: {e}"),
            })?
            .ok_or_else(|| IdentityError::SpireUnavailable {
                reason: format!("no trust bundle for trust domain {td}"),
            })?;
        let trust_bundle_pem = bundle
            .authorities()
            .iter()
            .map(|a| der_to_pem(a.as_bytes(), "CERTIFICATE"))
            .collect::<Vec<_>>()
            .join("\n");

        // Default expiry — 1 hour (actual expiry requires x509-parser which is heavy)
        let expires_at = chrono::Utc::now() + chrono::Duration::hours(1);

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
        tracing::debug!(
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
