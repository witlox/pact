//! Self-signed identity provider — CSR signing via journal CA.
//!
//! ADR-008 fallback: agent generates keypair + CSR, submits to journal,
//! receives signed cert. Used when SPIRE is not deployed.
//!
//! Note: actual CSR generation and journal RPC is handled by the
//! existing enrollment module. This provider wraps that functionality
//! behind the IdentityProvider trait.

use hpc_identity::{IdentityError, IdentityProvider, IdentitySource, WorkloadIdentity};
use tracing::{debug, info};

/// Self-signed identity provider (ADR-008 model).
pub struct SelfSignedProvider {
    /// Journal endpoint for CSR signing.
    journal_endpoint: String,
}

impl SelfSignedProvider {
    #[must_use]
    pub fn new(journal_endpoint: &str) -> Self {
        Self {
            journal_endpoint: journal_endpoint.to_string(),
        }
    }
}

#[async_trait::async_trait]
impl IdentityProvider for SelfSignedProvider {
    async fn get_identity(&self) -> Result<WorkloadIdentity, IdentityError> {
        info!(
            endpoint = %self.journal_endpoint,
            "requesting self-signed cert from journal CA"
        );

        // TODO: integrate with existing enrollment module
        // (crate::enrollment::generate_keypair_and_csr + EnrollmentService.Enroll RPC)
        //
        // For now, return an error. The cascade will fall through to bootstrap.
        Err(IdentityError::CsrSigningFailed {
            reason: "self-signed provider not yet wired to enrollment module".to_string(),
        })
    }

    async fn is_available(&self) -> bool {
        // Available if journal endpoint is configured (non-empty)
        let available = !self.journal_endpoint.is_empty();
        debug!(
            endpoint = %self.journal_endpoint,
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
    async fn self_signed_get_identity_not_yet_implemented() {
        let provider = SelfSignedProvider::new("https://journal:9443");
        let err = provider.get_identity().await.unwrap_err();
        assert!(matches!(err, IdentityError::CsrSigningFailed { .. }));
    }

    #[test]
    fn self_signed_source_type() {
        let provider = SelfSignedProvider::new("https://journal:9443");
        assert_eq!(provider.source_type(), IdentitySource::SelfSigned);
    }
}
