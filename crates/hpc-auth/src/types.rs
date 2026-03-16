use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Configuration for an [`AuthClient`](crate::AuthClient).
#[derive(Debug, Clone)]
pub struct AuthClientConfig {
    /// Server URL (e.g., "https://journal.example.com:9443").
    /// Used to key token cache and fetch auth discovery.
    pub server_url: String,

    /// Application name, used for cache directory (e.g., "pact" or "lattice").
    pub app_name: String,

    /// Permission mode for token cache files.
    pub permission_mode: PermissionMode,

    /// Override IdP configuration (skips server discovery).
    pub idp_override: Option<IdpConfig>,

    /// Force a specific OAuth2 flow.
    pub flow_override: Option<OAuthFlow>,

    /// Timeout for HTTP requests to IdP.
    pub timeout: Duration,
}

/// Permission mode for token cache file validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionMode {
    /// Reject cache with wrong permissions (pact default).
    Strict,
    /// Warn, fix permissions, proceed (lattice default).
    Lenient,
}

/// OAuth2 flow selection.
#[derive(Debug, Clone)]
pub enum OAuthFlow {
    /// Authorization Code with PKCE.
    AuthCodePkce,
    /// Device Authorization Grant.
    DeviceCode,
    /// Client Credentials (machine-to-machine).
    ClientCredentials { client_id: String, client_secret: String },
    /// Manual token paste (SSH sessions without browser).
    ManualPaste,
}

/// Override IdP configuration (skips server discovery).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdpConfig {
    pub issuer_url: String,
    pub client_id: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub revocation_endpoint: Option<String>,
    pub device_authorization_endpoint: Option<String>,
}

/// A set of OAuth2 tokens.
#[derive(Clone, Serialize, Deserialize)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub scopes: Vec<String>,
}

// Auth7: Never log refresh tokens.
impl fmt::Debug for TokenSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenSet")
            .field("access_token", &"[redacted]")
            .field("refresh_token", &self.refresh_token.as_ref().map(|_| "[redacted]"))
            .field("expires_at", &self.expires_at)
            .field("scopes", &self.scopes)
            .finish()
    }
}

impl fmt::Display for TokenSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TokenSet(expires_at={}, scopes={:?})", self.expires_at, self.scopes)
    }
}

/// Decoded JWT claims from an access token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenClaims {
    pub sub: String,
    pub iss: String,
    pub aud: StringOrVec,
    pub exp: u64,
    pub iat: u64,
    #[serde(default)]
    pub pact_role: Option<String>,
    #[serde(default)]
    pub pact_principal_type: Option<String>,
}

/// JWT `aud` claim can be a single string or a list.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StringOrVec {
    Single(String),
    Multiple(Vec<String>),
}

/// OIDC discovery document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcDiscovery {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    #[serde(default)]
    pub revocation_endpoint: Option<String>,
    #[serde(default)]
    pub device_authorization_endpoint: Option<String>,
    pub jwks_uri: String,
    #[serde(default)]
    pub grant_types_supported: Vec<String>,
    #[serde(default)]
    pub code_challenge_methods_supported: Vec<String>,
}

/// Cached discovery document with TTL metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedDiscovery {
    pub fetched_at: DateTime<Utc>,
    pub ttl_seconds: u64,
    pub document: OidcDiscovery,
}
