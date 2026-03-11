//! Typed error types for pact using `thiserror`.

use thiserror::Error;

/// Top-level error type for pact operations.
#[derive(Error, Debug)]
pub enum PactError {
    #[error("node not found: {0}")]
    NodeNotFound(String),

    #[error("vcluster not found: {0}")]
    VClusterNotFound(String),

    #[error("config entry not found: sequence {0}")]
    EntryNotFound(u64),

    #[error("commit window expired for node {node}: drift magnitude {magnitude}")]
    CommitWindowExpired { node: String, magnitude: f64 },

    #[error("authorization denied: {reason}")]
    Unauthorized { reason: String },

    #[error("policy evaluation failed: {0}")]
    PolicyError(String),

    #[error("journal unavailable: {0}")]
    JournalUnavailable(String),

    #[error("drift detected on node {node}: {detail}")]
    DriftDetected { node: String, detail: String },

    #[error("service {service} failed on node {node}: {reason}")]
    ServiceFailed { node: String, service: String, reason: String },

    #[error("emergency mode active on node {0}")]
    EmergencyActive(String),

    #[error("shell connection failed: {0}")]
    ShellError(String),

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("transport error: {0}")]
    Transport(#[from] tonic::Status),

    #[error("internal error: {0}")]
    Internal(String),
}
