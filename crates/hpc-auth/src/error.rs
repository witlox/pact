use thiserror::Error;

/// Errors returned by the hpc-auth library.
#[derive(Debug, Error)]
pub enum AuthError {
    /// IdP is unreachable (F15).
    #[error("IdP unreachable: {0}")]
    IdpUnreachable(String),

    /// No compatible OAuth2 flow available.
    #[error("no supported OAuth2 flow available from IdP discovery")]
    NoSupportedFlow,

    /// Token has expired and cannot be refreshed.
    #[error("token expired and no valid refresh token available")]
    TokenExpired,

    /// Cache file is corrupted (F16).
    #[error("token cache corrupted: {0}")]
    CacheCorrupted(String),

    /// Cache file has wrong permissions (strict mode).
    #[error("token cache permission denied: {0}")]
    CachePermissionDenied(String),

    /// OAuth2 exchange failed (invalid credentials, etc.).
    #[error("OAuth2 failed: {0}")]
    OAuthFailed(String),

    /// Timeout waiting for user action (browser callback, device code).
    #[error("authentication timed out")]
    Timeout,

    /// Discovery document is stale (F17).
    #[error("OIDC discovery document is stale")]
    StaleDiscovery,

    /// Internal error.
    #[error("internal error: {0}")]
    Internal(String),
}
