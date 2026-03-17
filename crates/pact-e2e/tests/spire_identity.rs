//! E2E test: SPIRE identity provider and cascade behavior.
//!
//! Tests the SpireProvider availability checks and cascade fallthrough
//! behavior. Full SVID acquisition requires the `spire` feature flag
//! on pact-agent and a running SPIRE server+agent pair.

use hpc_identity::{IdentityProvider, IdentitySource};
use pact_agent::identity_cascade::SpireProvider;

/// Test that SpireProvider correctly reports unavailable when no socket exists.
#[tokio::test]
async fn spire_provider_unavailable_without_socket() {
    let provider = SpireProvider::new("/nonexistent/spire.sock");
    assert!(!provider.is_available().await);
    assert_eq!(provider.source_type(), IdentitySource::Spire);
}

/// Test that SpireProvider reports unavailable without the spire feature.
/// (The stub impl always returns false for is_available.)
#[tokio::test]
async fn spire_provider_stub_always_unavailable() {
    let dir = tempfile::TempDir::new().unwrap();
    let socket_path = dir.path().join("agent.sock");
    std::fs::write(&socket_path, b"").unwrap();

    let provider = SpireProvider::new(socket_path.to_str().unwrap());
    // Without spire feature compiled into pact-agent, stub returns false
    // With spire feature, this would check path existence → true
    // pact-e2e doesn't enable the spire feature, so this is always the stub
    assert!(!provider.is_available().await);
}

/// Test the full identity cascade with bootstrap fallback.
///
/// This test verifies that when SPIRE is unavailable, the cascade
/// falls through to the bootstrap provider.
#[tokio::test]
async fn cascade_falls_through_to_bootstrap_when_spire_unavailable() {
    use hpc_identity::IdentityCascade;
    use pact_agent::identity_cascade::{BootstrapProvider, SelfSignedProvider};

    // Create bootstrap cert files
    let dir = tempfile::TempDir::new().unwrap();
    let cert = dir.path().join("cert.pem");
    let key = dir.path().join("key.pem");
    let ca = dir.path().join("ca.pem");
    std::fs::write(&cert, b"BOOTSTRAP CERT").unwrap();
    std::fs::write(&key, b"BOOTSTRAP KEY").unwrap();
    std::fs::write(&ca, b"BOOTSTRAP CA").unwrap();

    let cascade = IdentityCascade::new(vec![
        // SPIRE — unavailable (no socket)
        Box::new(SpireProvider::new("/nonexistent/spire.sock")),
        // SelfSigned — no cached enrollment
        Box::new(SelfSignedProvider::new("https://journal:9443")),
        // Bootstrap — has cert files
        Box::new(BootstrapProvider::new(
            cert.to_str().unwrap(),
            key.to_str().unwrap(),
            ca.to_str().unwrap(),
        )),
    ]);

    assert_eq!(cascade.provider_count(), 3);

    let identity = cascade.get_identity().await.unwrap();
    // Should have fallen through to bootstrap
    assert_eq!(identity.source, IdentitySource::Bootstrap);
    assert_eq!(identity.cert_chain_pem, b"BOOTSTRAP CERT");
}
