//! SPIRE identity provider — obtains SVID from SPIRE agent.
//!
//! Connects to the SPIRE Workload API via unix socket.
//! Primary identity source when SPIRE is deployed (A-I7).
//!
//! Note: actual SPIRE Workload API integration requires the
//! spiffe crate or direct protobuf client. This is a stub that
//! checks socket availability and will be completed when the
//! SPIRE integration is fully designed.

use hpc_identity::{IdentityError, IdentityProvider, IdentitySource, WorkloadIdentity};
use tracing::{debug, info};

/// SPIRE identity provider.
pub struct SpireProvider {
    /// Path to the SPIRE agent Workload API socket.
    agent_socket: String,
}

impl SpireProvider {
    #[must_use]
    pub fn new(agent_socket: &str) -> Self {
        Self {
            agent_socket: agent_socket.to_string(),
        }
    }
}

#[async_trait::async_trait]
impl IdentityProvider for SpireProvider {
    async fn get_identity(&self) -> Result<WorkloadIdentity, IdentityError> {
        info!(socket = %self.agent_socket, "requesting SVID from SPIRE agent");

        // TODO: implement actual SPIRE Workload API client
        // For now, return an error indicating SPIRE is not yet implemented.
        // The cascade will fall through to the next provider.
        Err(IdentityError::SpireUnavailable {
            reason: "SPIRE Workload API client not yet implemented".to_string(),
        })
    }

    async fn is_available(&self) -> bool {
        // Check if the SPIRE agent socket exists
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
