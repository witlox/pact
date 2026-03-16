# hpc-auth Crate Interface

Shared OAuth2/OIDC authentication library consumed by pact-cli and lattice-cli.

## Module Boundary

`hpc-auth` is a **library crate** (no binary). It owns:
- OAuth2 flow execution (PKCE, Device Code, Client Credentials, Manual Paste)
- Token cache (read/write/validate/clear)
- OIDC discovery document fetching and caching
- Token refresh (silent + interactive)

It does NOT own:
- gRPC metadata injection (consumer's responsibility)
- RBAC/policy evaluation (server-side)
- User-facing CLI argument parsing (consumer defines `login`/`logout` subcommands)

## Public Interface

```rust
// --- Core types ---

pub struct AuthClient {
    config: AuthClientConfig,
    cache: TokenCache,
    discovery: DiscoveryCache,
}

pub struct AuthClientConfig {
    /// Server URL (e.g., "https://journal.example.com:9443").
    /// Used to key token cache and fetch auth discovery.
    pub server_url: String,
    /// Permission mode for token cache files.
    pub permission_mode: PermissionMode,
    /// Override IdP configuration (skips server discovery).
    pub idp_override: Option<IdpConfig>,
    /// Force a specific OAuth2 flow.
    pub flow_override: Option<OAuthFlow>,
    /// Timeout for HTTP requests to IdP.
    pub timeout: Duration,
}

pub enum PermissionMode {
    /// Reject cache with wrong permissions (PACT default).
    Strict,
    /// Warn, fix permissions, proceed (Lattice default).
    Lenient,
}

pub enum OAuthFlow {
    AuthCodePkce,
    DeviceCode,
    ClientCredentials { client_id: String, client_secret: String },
    ManualPaste,
}

pub struct IdpConfig {
    pub issuer_url: String,
    pub client_id: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub revocation_endpoint: Option<String>,
    pub device_authorization_endpoint: Option<String>,
}

// --- Token types ---

pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub scopes: Vec<String>,
}

pub struct TokenClaims {
    pub sub: String,
    pub iss: String,
    pub aud: StringOrVec,
    pub exp: u64,
    pub iat: u64,
    pub pact_role: Option<String>,
    pub pact_principal_type: Option<String>,
}

// --- Error types ---

pub enum AuthError {
    /// IdP is unreachable (F15).
    IdpUnreachable(String),
    /// No compatible OAuth2 flow available.
    NoSupportedFlow,
    /// Token has expired and cannot be refreshed.
    TokenExpired,
    /// Cache file is corrupted (F16).
    CacheCorrupted(String),
    /// Cache file has wrong permissions (strict mode).
    CachePermissionDenied(String),
    /// OAuth2 exchange failed (invalid credentials, etc.).
    OAuthFailed(String),
    /// Timeout waiting for user action (browser callback, device code).
    Timeout,
    /// Discovery document is stale (F17).
    StaleDiscovery,
}

// --- Core operations ---

impl AuthClient {
    /// Create a new auth client with the given configuration.
    pub fn new(config: AuthClientConfig) -> Result<Self, AuthError>;

    /// Initiate login. Returns the token set on success.
    /// CONTRACT: Selects flow per cascade (Auth8). Opens browser or prints
    /// device code URL. Stores tokens in cache with correct permissions (Auth5).
    pub async fn login(&self) -> Result<TokenSet, AuthError>;

    /// Logout. Revokes refresh token at IdP, clears cache (Auth4).
    /// CONTRACT: Always clears local cache regardless of IdP revocation result.
    pub async fn logout(&self) -> Result<(), AuthError>;

    /// Get a valid access token. Refreshes silently if expired (Auth3).
    /// CONTRACT: Returns cached token if valid. Refreshes if expired but
    /// refresh token valid. Returns error if both expired.
    pub async fn get_token(&self) -> Result<String, AuthError>;

    /// Check if a valid token exists without refreshing.
    pub fn is_logged_in(&self) -> bool;

    /// Get the server URL this client targets.
    pub fn server_url(&self) -> &str;
}

// --- Token cache ---

pub struct TokenCache {
    // Internal: file-based, per-server keyed (Auth6)
}

impl TokenCache {
    /// Read tokens for a specific server.
    /// CONTRACT: Validates file permissions per PermissionMode (Auth5).
    /// Returns CacheCorrupted if file is not valid JSON (Auth2).
    pub fn read(&self, server_url: &str) -> Result<Option<TokenSet>, AuthError>;

    /// Write tokens for a specific server.
    /// CONTRACT: Creates file with 0600 permissions. Updates existing entry
    /// if present. Never logs refresh tokens (Auth7).
    pub fn write(&self, server_url: &str, tokens: &TokenSet) -> Result<(), AuthError>;

    /// Delete tokens for a specific server.
    pub fn delete(&self, server_url: &str) -> Result<(), AuthError>;

    /// List servers with cached tokens.
    pub fn list_servers(&self) -> Vec<String>;

    /// Get/set the default server.
    pub fn default_server(&self) -> Option<String>;
    pub fn set_default_server(&self, server_url: &str) -> Result<(), AuthError>;
}

// --- Discovery ---

pub struct DiscoveryCache {
    // Internal: caches OIDC discovery documents per issuer
}

impl DiscoveryCache {
    /// Fetch discovery document for an issuer.
    /// CONTRACT: Returns cached document if fresh. Fetches from
    /// {issuer}/.well-known/openid-configuration if stale or missing.
    /// On fetch failure, returns stale cached document (degraded mode).
    pub async fn get(&self, issuer_url: &str) -> Result<OidcDiscovery, AuthError>;

    /// Clear cached discovery document (on auth failure suggesting staleness).
    pub fn clear(&self, issuer_url: &str);
}

pub struct OidcDiscovery {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub revocation_endpoint: Option<String>,
    pub device_authorization_endpoint: Option<String>,
    pub jwks_uri: String,
    pub grant_types_supported: Vec<String>,
    pub code_challenge_methods_supported: Vec<String>,
}
```

## Consumer Integration Pattern

```rust
// PACT CLI consumer example (NOT in hpc-auth crate):
let auth = AuthClient::new(AuthClientConfig {
    server_url: config.endpoint.clone(),
    permission_mode: PermissionMode::Strict, // PACT: strict (PAuth1)
    idp_override: None,
    flow_override: None,
    timeout: Duration::from_secs(30),
})?;

// Login subcommand
auth.login().await?;

// Any authenticated command
let token = auth.get_token().await?;
let mut request = tonic::Request::new(my_payload);
request.metadata_mut().insert(
    "authorization",
    format!("Bearer {token}").parse()?,
);
client.some_rpc(request).await?;
```

## Invariant Enforcement Points

| Invariant | Enforcement Location |
|-----------|---------------------|
| Auth1: No unauth commands | `get_token()` returns error → consumer exits |
| Auth2: Fail closed on corruption | `TokenCache::read()` returns `CacheCorrupted` |
| Auth3: Concurrent refresh safe | `get_token()` uses file lock, idempotent refresh |
| Auth4: Logout always clears | `logout()` deletes cache before IdP revocation |
| Auth5: Cache 0600 permissions | `TokenCache::read()`/`write()` validate permissions |
| Auth6: Per-server isolation | `TokenCache` keys by `server_url` |
| Auth7: No refresh token logging | `TokenSet::fmt()` redacts refresh_token |
| Auth8: Cascading flow fallback | `login()` tries flows in order based on discovery |
