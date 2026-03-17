//! Self-signed identity provider — CSR signing via journal CA.
//!
//! ADR-008 fallback: agent generates keypair + CSR, submits to journal,
//! receives signed cert. Used when SPIRE is not deployed.
//!
//! Wraps the existing enrollment module's keypair generation and CSR
//! submission behind the `IdentityProvider` trait.

use hpc_identity::{IdentityError, IdentityProvider, IdentitySource, WorkloadIdentity};
use tracing::{debug, info};

use crate::enrollment::EnrollmentResult;

/// Self-signed identity provider (ADR-008 model).
///
/// Uses the enrollment module to generate keypairs and submit CSRs
/// to the journal CA. The journal signs locally with its intermediate
/// CA key (local signing, no external dependency).
pub struct SelfSignedProvider {
    /// Journal endpoint for CSR signing.
    journal_endpoint: String,
    /// Pre-computed enrollment result (if available from boot enrollment).
    cached_result: tokio::sync::Mutex<Option<EnrollmentResult>>,
}

impl SelfSignedProvider {
    #[must_use]
    pub fn new(journal_endpoint: &str) -> Self {
        Self {
            journal_endpoint: journal_endpoint.to_string(),
            cached_result: tokio::sync::Mutex::new(None),
        }
    }

    /// Provide a pre-computed enrollment result (from boot enrollment).
    ///
    /// This avoids re-enrolling when the enrollment module has already
    /// obtained a cert during the boot sequence.
    pub async fn set_enrollment_result(&self, result: EnrollmentResult) {
        *self.cached_result.lock().await = Some(result);
    }
}

#[async_trait::async_trait]
impl IdentityProvider for SelfSignedProvider {
    async fn get_identity(&self) -> Result<WorkloadIdentity, IdentityError> {
        // Check for cached enrollment result first
        let cached = self.cached_result.lock().await;
        if let Some(ref result) = *cached {
            info!("using cached enrollment result for identity");
            let expires_at = chrono::DateTime::parse_from_rfc3339(&result.cert_expires_at)
                .map_or_else(
                    |_| chrono::Utc::now() + chrono::Duration::days(3),
                    |dt| dt.with_timezone(&chrono::Utc),
                );

            return Ok(WorkloadIdentity {
                cert_chain_pem: result.cert_pem.clone(),
                private_key_pem: result.key_pair_pem.as_bytes().to_vec(),
                trust_bundle_pem: result.ca_chain_pem.clone(),
                expires_at,
                source: IdentitySource::SelfSigned,
            });
        }
        drop(cached);

        // No cached result — would need to run enrollment
        // This requires the enrollment module's full flow (hardware identity + CSR + RPC)
        // which is an async operation requiring journal connectivity.
        info!(
            endpoint = %self.journal_endpoint,
            "self-signed provider: no cached enrollment, journal enrollment needed"
        );
        Err(IdentityError::CsrSigningFailed {
            reason: "no cached enrollment result and live enrollment not yet implemented in cascade".to_string(),
        })
    }

    async fn is_available(&self) -> bool {
        // Available if we have a cached result OR journal endpoint is configured
        let has_cached = self.cached_result.lock().await.is_some();
        let has_endpoint = !self.journal_endpoint.is_empty();
        let available = has_cached || has_endpoint;
        debug!(
            endpoint = %self.journal_endpoint,
            has_cached = has_cached,
            available = available,
            "self-signed provider availability check"
        );
        available
    }

    fn source_type(&self) -> IdentitySource {
        IdentitySource::SelfSigned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn self_signed_available_with_endpoint() {
        let provider = SelfSignedProvider::new("https://journal:9443");
        assert!(provider.is_available().await);
    }

    #[tokio::test]
    async fn self_signed_not_available_empty_endpoint() {
        let provider = SelfSignedProvider::new("");
        assert!(!provider.is_available().await);
    }

    #[tokio::test]
    async fn self_signed_no_cached_result_fails() {
        let provider = SelfSignedProvider::new("https://journal:9443");
        let err = provider.get_identity().await.unwrap_err();
        assert!(matches!(err, IdentityError::CsrSigningFailed { .. }));
    }

    #[tokio::test]
    async fn self_signed_with_cached_enrollment() {
        let provider = SelfSignedProvider::new("https://journal:9443");

        let result = EnrollmentResult {
            node_id: "node-001".into(),
            domain_id: "test-domain".into(),
            vcluster_id: None,
            cert_pem: b"CERT".to_vec(),
            ca_chain_pem: b"CA".to_vec(),
            cert_serial: "serial-123".into(),
            cert_expires_at: "2026-03-20T00:00:00Z".into(),
            key_pair_pem: "KEY".into(),
        };
        provider.set_enrollment_result(result).await;

        assert!(provider.is_available().await);
        let id = provider.get_identity().await.unwrap();
        assert_eq!(id.source, IdentitySource::SelfSigned);
        assert_eq!(id.cert_chain_pem, b"CERT");
    }

    #[test]
    fn self_signed_source_type() {
        let provider = SelfSignedProvider::new("https://journal:9443");
        assert_eq!(provider.source_type(), IdentitySource::SelfSigned);
    }
}
