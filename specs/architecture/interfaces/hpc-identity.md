# hpc-identity Interface Definitions

Shared contract crate in hpc-core. Abstracts workload identity acquisition (SPIRE/self-signed/bootstrap) and certificate rotation. Both pact and lattice implement.

**Source:** assumptions.md A-mTLS1, domain-model.md §2e (Platform Bootstrap), interaction N10
**Invariants:** PB4, PB5, E4-E6
**Note:** Partially supersedes ADR-008 cert management model. Enrollment registry and state machine survive.

---

## Workload Identity

```rust
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};

/// Source-agnostic workload identity.
/// Contains everything needed to establish an mTLS connection.
/// Source: domain-model.md hpc-identity shared types
#[derive(Debug, Clone)]
pub struct WorkloadIdentity {
    /// Certificate chain (PEM). Leaf cert + intermediates.
    pub cert_chain_pem: Vec<u8>,
    /// Private key (PEM). Never logged, never transmitted.
    pub private_key_pem: Vec<u8>,
    /// Trust bundle (PEM). CA certs for verifying peers.
    pub trust_bundle_pem: Vec<u8>,
    /// When this identity expires
    pub expires_at: DateTime<Utc>,
    /// Where this identity came from (for audit)
    pub source: IdentitySource,
}

/// How the identity was obtained.
/// Source: domain-model.md IdentitySource
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IdentitySource {
    /// SPIRE SVID via Workload API
    Spire,
    /// Self-signed via journal/quorum intermediate CA (ADR-008 fallback)
    SelfSigned,
    /// Bootstrap cert from OpenCHAMI provisioning
    Bootstrap,
}

impl WorkloadIdentity {
    /// Check if identity is still valid (not expired).
    pub fn is_valid(&self) -> bool {
        // CONTRACT: returns true if expires_at > now
        todo!()
    }

    /// Check if identity should be renewed (2/3 of lifetime elapsed).
    /// Source: invariant E5 (renewal at 2/3 of lifetime)
    pub fn should_renew(&self) -> bool {
        // CONTRACT: returns true if remaining lifetime < 1/3 of total
        todo!()
    }
}
```

---

## Identity Provider Trait

```rust
/// Trait for obtaining workload identity.
/// Implementations: SpireProvider, SelfSignedProvider, StaticProvider.
/// Source: domain-model.md hpc-identity IdentityProvider
///
/// CONTRACT:
/// - get_identity() must return a valid WorkloadIdentity or an error.
/// - Implementations handle their own retry logic.
/// - Private keys are generated locally, never transmitted.
/// - Source field in returned identity must accurately reflect provenance.
#[async_trait::async_trait]
pub trait IdentityProvider: Send + Sync {
    /// Obtain a workload identity.
    /// May involve network calls (SPIRE socket, journal CSR signing).
    async fn get_identity(&self) -> Result<WorkloadIdentity, IdentityError>;

    /// Check if this provider is available.
    /// E.g., SpireProvider checks if SPIRE agent socket exists.
    async fn is_available(&self) -> bool;

    /// The source type this provider produces.
    fn source_type(&self) -> IdentitySource;
}
```

---

## Certificate Rotator Trait

```rust
/// Trait for certificate rotation.
/// Default implementation: dual-channel swap pattern (ADR-008, E6).
/// Source: invariant E6 (dual-channel rotation)
///
/// CONTRACT:
/// - rotate() must not interrupt in-flight operations.
/// - Old channel drains before being dropped.
/// - If rotation fails, active channel continues.
#[async_trait::async_trait]
pub trait CertRotator: Send + Sync {
    /// Rotate to a new identity.
    /// Builds passive channel, health-checks, atomically swaps.
    /// Source: interaction N10, invariant E6
    async fn rotate(&self, new_identity: WorkloadIdentity) -> Result<(), IdentityError>;
}
```

---

## Identity Cascade

```rust
/// Cascading identity provider.
/// Tries providers in order: SPIRE → SelfSigned → Bootstrap.
/// Source: assumption A-mTLS1, invariant PB5 (no hard SPIRE dependency)
///
/// CONTRACT:
/// - Tries each provider in order until one succeeds.
/// - Records which provider succeeded (for audit via IdentitySource).
/// - If all fail, returns last error.
/// - On first success, does NOT retry earlier providers within same call.
///   Rotation to better provider (e.g., SPIRE becomes available later)
///   is handled by periodic renewal, not by the cascade.
pub struct IdentityCascade {
    providers: Vec<Box<dyn IdentityProvider>>,
}

impl IdentityCascade {
    pub fn new(providers: Vec<Box<dyn IdentityProvider>>) -> Self {
        Self { providers }
    }

    /// Get identity from the first available provider.
    pub async fn get_identity(&self) -> Result<WorkloadIdentity, IdentityError> {
        // CONTRACT: try each provider in order, return first success
        todo!()
    }
}
```

---

## Provider Configurations

```rust
/// Configuration for SPIRE provider.
/// Source: assumption A-I7 (SPIRE pre-existing)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpireConfig {
    /// Path to SPIRE agent socket
    /// Source: open unknown (SPIRE socket path on HPE Cray)
    pub agent_socket: String,
    /// SPIFFE ID to request
    pub spiffe_id: Option<String>,
    /// Timeout for SVID acquisition
    pub timeout_seconds: u64,
}

impl Default for SpireConfig {
    fn default() -> Self {
        Self {
            agent_socket: "/run/spire/agent.sock".to_string(),
            spiffe_id: None, // auto-detect from attestation
            timeout_seconds: 30,
        }
    }
}

/// Configuration for self-signed provider (ADR-008 fallback).
/// Source: ADR-008, invariants E4-E5
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelfSignedConfig {
    /// Journal/quorum endpoint for CSR signing
    pub signing_endpoint: String,
    /// Certificate lifetime in seconds (default 3 days)
    pub cert_lifetime_seconds: u64,
}

impl Default for SelfSignedConfig {
    fn default() -> Self {
        Self {
            signing_endpoint: String::new(),
            cert_lifetime_seconds: 259200, // 3 days
        }
    }
}

/// Configuration for bootstrap/static provider.
/// Source: domain-model.md §2e BootstrapIdentity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapConfig {
    /// Path to bootstrap cert (in SquashFS image or tmpfs)
    pub cert_path: String,
    /// Path to bootstrap private key
    pub key_path: String,
    /// Path to trust bundle
    pub trust_bundle_path: String,
}
```

---

## Error Types

```rust
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("SPIRE agent unavailable: {reason}")]
    SpireUnavailable { reason: String },
    #[error("CSR signing failed: {reason}")]
    CsrSigningFailed { reason: String },
    #[error("bootstrap identity not found: {path}")]
    BootstrapNotFound { path: String },
    #[error("identity expired")]
    Expired,
    #[error("rotation failed: {reason}")]
    RotationFailed { reason: String },
    #[error("no identity provider available")]
    NoProviderAvailable,
    #[error("identity I/O error: {source}")]
    Io {
        #[from]
        source: std::io::Error,
    },
}
```
