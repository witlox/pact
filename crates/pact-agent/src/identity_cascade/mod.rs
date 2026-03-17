//! Identity cascade — SPIRE/SelfSigned/Bootstrap provider implementations.
//!
//! Implements `hpc_identity::IdentityProvider` for each identity source.
//! The cascade tries providers in priority order (PB5: no hard SPIRE dependency).
//!
//! # Provider priority
//!
//! 1. SpireProvider — connects to SPIRE agent socket, obtains SVID
//! 2. SelfSignedProvider — generates CSR, journal signs (ADR-008 fallback)
//! 3. BootstrapProvider — reads cert from filesystem (initial boot)

mod bootstrap;
mod self_signed;
mod spire;

pub use bootstrap::BootstrapProvider;
pub use self_signed::SelfSignedProvider;
pub use spire::SpireProvider;

use hpc_identity::{IdentityCascade, IdentityProvider};

/// Build the default identity cascade for pact-agent.
///
/// Order: SPIRE → SelfSigned → Bootstrap.
/// Each provider that is configured is included; unconfigured ones are skipped.
#[must_use]
pub fn build_cascade(
    spire_socket: Option<String>,
    journal_endpoint: Option<String>,
    bootstrap_cert: Option<String>,
    bootstrap_key: Option<String>,
    bootstrap_ca: Option<String>,
) -> IdentityCascade {
    let mut providers: Vec<Box<dyn IdentityProvider>> = Vec::new();

    // SPIRE provider (highest priority)
    if let Some(socket) = spire_socket {
        providers.push(Box::new(SpireProvider::new(&socket)));
    }

    // Self-signed provider (ADR-008 fallback)
    if let Some(endpoint) = journal_endpoint {
        providers.push(Box::new(SelfSignedProvider::new(&endpoint)));
    }

    // Bootstrap provider (last resort)
    if let (Some(cert), Some(key), Some(ca)) = (bootstrap_cert, bootstrap_key, bootstrap_ca) {
        providers.push(Box::new(BootstrapProvider::new(&cert, &key, &ca)));
    }

    IdentityCascade::new(providers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_cascade_with_all_providers() {
        let cascade = build_cascade(
            Some("/run/spire/agent.sock".into()),
            Some("https://journal:9443".into()),
            Some("/etc/pact/cert.pem".into()),
            Some("/etc/pact/key.pem".into()),
            Some("/etc/pact/ca.pem".into()),
        );
        assert_eq!(cascade.provider_count(), 3);
    }

    #[test]
    fn build_cascade_bootstrap_only() {
        let cascade = build_cascade(
            None,
            None,
            Some("/etc/pact/cert.pem".into()),
            Some("/etc/pact/key.pem".into()),
            Some("/etc/pact/ca.pem".into()),
        );
        assert_eq!(cascade.provider_count(), 1);
    }

    #[test]
    fn build_cascade_empty() {
        let cascade = build_cascade(None, None, None, None, None);
        assert_eq!(cascade.provider_count(), 0);
    }
}
