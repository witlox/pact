//! OIDC token extraction and validation from gRPC metadata.
//!
//! Tokens arrive in gRPC metadata: `authorization: Bearer <token>`.
//! Validation:
//! 1. Extract token from metadata
//! 2. Decode JWT header to find key ID (kid)
//! 3. Validate signature against cached JWKS
//! 4. Check expiry, audience, issuer
//!
//! Degraded mode (P7): if JWKS refresh fails, use cached keys.

use std::sync::Arc;
use std::time::{Duration, Instant};

use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{debug, warn};

use pact_common::types::{Identity, PrincipalType};

/// Extracted claims from a validated OIDC token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenClaims {
    /// Subject (user email or service principal).
    pub sub: String,
    /// Audience.
    #[serde(default)]
    pub aud: StringOrVec,
    /// Issuer.
    #[serde(default)]
    pub iss: String,
    /// Expiry (unix timestamp).
    #[serde(default)]
    pub exp: u64,
    /// Issued at (unix timestamp).
    #[serde(default)]
    pub iat: u64,
    /// Pact role claim (custom claim).
    #[serde(default, rename = "pact_role")]
    pub pact_role: Option<String>,
    /// Principal type claim.
    #[serde(default, rename = "pact_principal_type")]
    pub pact_principal_type: Option<String>,
}

/// JWT `aud` can be a string or array of strings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StringOrVec {
    #[default]
    None,
    Single(String),
    Multiple(Vec<String>),
}

impl StringOrVec {
    pub fn contains(&self, value: &str) -> bool {
        match self {
            Self::None => false,
            Self::Single(s) => s == value,
            Self::Multiple(v) => v.iter().any(|s| s == value),
        }
    }
}

/// Configuration for OIDC token validation.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Expected issuer (e.g., "https://auth.example.com").
    pub issuer: String,
    /// Expected audience (e.g., "pact-agent").
    pub audience: String,
    /// HMAC secret for development/testing (production uses JWKS RSA keys).
    pub hmac_secret: Option<Vec<u8>>,
    /// JWKS endpoint URL for RS256 validation (e.g., "https://auth.example.com/.well-known/jwks.json").
    /// If None, derived from issuer as `{issuer}/.well-known/jwks.json`.
    pub jwks_url: Option<String>,
}

/// A single JSON Web Key from a JWKS endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Jwk {
    /// Key type (e.g., "RSA").
    pub kty: String,
    /// Key ID.
    #[serde(default)]
    pub kid: Option<String>,
    /// Algorithm (e.g., "RS256").
    #[serde(default)]
    pub alg: Option<String>,
    /// RSA modulus (Base64url-encoded).
    #[serde(default)]
    pub n: Option<String>,
    /// RSA exponent (Base64url-encoded).
    #[serde(default)]
    pub e: Option<String>,
    /// Key use (e.g., "sig").
    #[serde(default, rename = "use")]
    pub key_use: Option<String>,
}

/// JWKS response from the OIDC provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwksResponse {
    pub keys: Vec<Jwk>,
}

/// Cache for JWKS keys fetched from an OIDC provider.
///
/// Keys are cached for 1 hour to avoid hitting the JWKS endpoint on every request.
/// In degraded mode (P7), stale cached keys are used if a refresh fails.
#[derive(Debug, Clone)]
pub struct JwksCache {
    inner: Arc<RwLock<JwksCacheInner>>,
    ttl: Duration,
}

#[derive(Debug)]
struct JwksCacheInner {
    keys: Vec<Jwk>,
    fetched_at: Option<Instant>,
}

impl JwksCache {
    /// Create a new JWKS cache with the default TTL of 1 hour.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(JwksCacheInner { keys: Vec::new(), fetched_at: None })),
            ttl: Duration::from_secs(3600),
        }
    }

    /// Create a JWKS cache with a custom TTL (useful for testing).
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            inner: Arc::new(RwLock::new(JwksCacheInner { keys: Vec::new(), fetched_at: None })),
            ttl,
        }
    }

    /// Fetch JWKS keys from the given URL, using the cache if still valid.
    ///
    /// If the fetch fails and cached keys exist, returns the stale cached keys
    /// (degraded mode per P7).
    pub async fn fetch(&self, jwks_url: &str) -> Result<Vec<Jwk>, AuthError> {
        // Check if cache is still valid.
        {
            let inner = self.inner.read().await;
            if let Some(fetched_at) = inner.fetched_at {
                if fetched_at.elapsed() < self.ttl && !inner.keys.is_empty() {
                    return Ok(inner.keys.clone());
                }
            }
        }

        // Cache miss or expired — fetch fresh keys.
        match self.fetch_remote(jwks_url).await {
            Ok(keys) => {
                let mut inner = self.inner.write().await;
                inner.keys.clone_from(&keys);
                inner.fetched_at = Some(Instant::now());
                Ok(keys)
            }
            Err(e) => {
                // Degraded mode: return stale cached keys if available.
                let inner = self.inner.read().await;
                if inner.keys.is_empty() {
                    Err(e)
                } else {
                    warn!(
                        error = %e,
                        "JWKS refresh failed, using stale cached keys (degraded mode)"
                    );
                    Ok(inner.keys.clone())
                }
            }
        }
    }

    /// Return the currently cached keys without fetching.
    pub async fn cached_keys(&self) -> Vec<Jwk> {
        self.inner.read().await.keys.clone()
    }

    /// Directly set cached keys (useful for testing or pre-loading).
    pub async fn set_keys(&self, keys: Vec<Jwk>) {
        let mut inner = self.inner.write().await;
        inner.keys = keys;
        inner.fetched_at = Some(Instant::now());
    }

    /// Perform the HTTP fetch of the JWKS endpoint.
    async fn fetch_remote(&self, jwks_url: &str) -> Result<Vec<Jwk>, AuthError> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| AuthError::JwksFetchError(e.to_string()))?;

        let resp = client
            .get(jwks_url)
            .send()
            .await
            .map_err(|e| AuthError::JwksFetchError(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(AuthError::JwksFetchError(format!(
                "JWKS endpoint returned HTTP {}",
                resp.status()
            )));
        }

        let jwks: JwksResponse =
            resp.json().await.map_err(|e| AuthError::JwksFetchError(e.to_string()))?;

        Ok(jwks.keys)
    }
}

impl Default for JwksCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Extract Bearer token from gRPC metadata value.
pub fn extract_bearer_token(metadata_value: &str) -> Option<&str> {
    metadata_value.strip_prefix("Bearer ").or_else(|| metadata_value.strip_prefix("bearer "))
}

/// Validate a JWT token and return the claims (synchronous, HS256 only).
///
/// For development/testing with HMAC (HS256) shared secret.
/// For production RS256/JWKS validation, use `validate_token_with_jwks`.
pub fn validate_token(token: &str, config: &AuthConfig) -> Result<TokenClaims, AuthError> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_audience(&[&config.audience]);
    validation.set_issuer(&[&config.issuer]);
    validation.validate_exp = true;

    let secret = config.hmac_secret.as_deref().ok_or(AuthError::NoSigningKey)?;
    let key = DecodingKey::from_secret(secret);

    let token_data = decode::<TokenClaims>(token, &key, &validation).map_err(|e| {
        debug!(error = %e, "Token validation failed");
        map_jwt_error(&e, config)
    })?;

    Ok(token_data.claims)
}

/// Validate a JWT token with JWKS fallback for RS256 (async, production use).
///
/// Strategy:
/// 1. If `hmac_secret` is set, try HS256 first (dev mode).
/// 2. If HS256 fails or no secret is configured, try RS256 with JWKS.
/// 3. Match the token's `kid` header against the JWKS keys.
pub async fn validate_token_with_jwks(
    token: &str,
    config: &AuthConfig,
    jwks_cache: &JwksCache,
) -> Result<TokenClaims, AuthError> {
    // Try HS256 first if a secret is available.
    if config.hmac_secret.is_some() {
        match validate_token(token, config) {
            Ok(claims) => return Ok(claims),
            Err(e) => {
                debug!(error = %e, "HS256 validation failed, trying RS256 with JWKS");
            }
        }
    }

    // RS256 with JWKS.
    let jwks_url = config.jwks_url.clone().unwrap_or_else(|| {
        let issuer = config.issuer.trim_end_matches('/');
        format!("{issuer}/.well-known/jwks.json")
    });

    let keys = jwks_cache.fetch(&jwks_url).await?;
    if keys.is_empty() {
        return Err(AuthError::NoSigningKey);
    }

    // Decode the JWT header to find the kid.
    let header =
        jsonwebtoken::decode_header(token).map_err(|e| AuthError::InvalidToken(e.to_string()))?;

    let token_kid = header.kid.as_deref();

    // Find matching key: by kid if present, otherwise try each RSA key.
    let matching_keys: Vec<&Jwk> = keys
        .iter()
        .filter(|k| k.kty == "RSA")
        .filter(|k| k.n.is_some() && k.e.is_some())
        .filter(|k| {
            // If token has a kid, match it; otherwise try all RSA keys.
            match (token_kid, k.kid.as_deref()) {
                (Some(tk), Some(kk)) => tk == kk,
                (Some(_), None) => false,
                (None, _) => true,
            }
        })
        .collect();

    if matching_keys.is_empty() {
        return Err(AuthError::InvalidToken(format!("no matching JWKS key for kid {token_kid:?}")));
    }

    let mut last_err = AuthError::NoSigningKey;
    for jwk in matching_keys {
        let n = jwk.n.as_ref().unwrap();
        let e = jwk.e.as_ref().unwrap();

        let decoding_key = match DecodingKey::from_rsa_components(n, e) {
            Ok(k) => k,
            Err(err) => {
                debug!(error = %err, kid = ?jwk.kid, "Failed to build RSA key from components");
                last_err = AuthError::InvalidToken(err.to_string());
                continue;
            }
        };

        let mut validation = Validation::new(Algorithm::RS256);
        validation.set_audience(&[&config.audience]);
        validation.set_issuer(&[&config.issuer]);
        validation.validate_exp = true;

        match decode::<TokenClaims>(token, &decoding_key, &validation) {
            Ok(token_data) => return Ok(token_data.claims),
            Err(e) => {
                debug!(error = %e, kid = ?jwk.kid, "RS256 validation failed with this key");
                last_err = map_jwt_error(&e, config);
            }
        }
    }

    Err(last_err)
}

/// Map jsonwebtoken errors to `AuthError` variants.
fn map_jwt_error(e: &jsonwebtoken::errors::Error, config: &AuthConfig) -> AuthError {
    match e.kind() {
        jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::TokenExpired,
        jsonwebtoken::errors::ErrorKind::InvalidAudience => {
            AuthError::InvalidAudience(config.audience.clone())
        }
        jsonwebtoken::errors::ErrorKind::InvalidIssuer => {
            AuthError::InvalidIssuer(config.issuer.clone())
        }
        _ => AuthError::InvalidToken(e.to_string()),
    }
}

/// Convert validated claims to a pact Identity.
pub fn claims_to_identity(claims: &TokenClaims) -> Identity {
    let principal_type = match claims.pact_principal_type.as_deref() {
        Some("agent") => PrincipalType::Agent,
        Some("service") => PrincipalType::Service,
        _ => PrincipalType::Human,
    };

    Identity {
        principal: claims.sub.clone(),
        principal_type,
        role: claims.pact_role.clone().unwrap_or_default(),
    }
}

/// Check if an identity has platform admin privileges.
pub fn is_platform_admin(identity: &Identity) -> bool {
    identity.role == "pact-platform-admin"
}

/// Check if an identity has ops privileges for a given vCluster.
pub fn has_ops_role(identity: &Identity, vcluster: &str) -> bool {
    identity.role == "pact-platform-admin" || identity.role == format!("pact-ops-{vcluster}")
}

/// Check if an identity has viewer privileges for a given vCluster.
pub fn has_viewer_role(identity: &Identity, vcluster: &str) -> bool {
    has_ops_role(identity, vcluster) || identity.role == format!("pact-viewer-{vcluster}")
}

/// Authentication errors.
#[derive(Debug, Clone, thiserror::Error)]
pub enum AuthError {
    #[error("missing authorization metadata")]
    MissingToken,
    #[error("invalid Bearer token format")]
    InvalidFormat,
    #[error("token expired")]
    TokenExpired,
    #[error("invalid audience: expected {0}")]
    InvalidAudience(String),
    #[error("invalid issuer: expected {0}")]
    InvalidIssuer(String),
    #[error("invalid token: {0}")]
    InvalidToken(String),
    #[error("no signing key configured")]
    NoSigningKey,
    #[error("JWKS fetch error: {0}")]
    JwksFetchError(String),
    #[error("insufficient privileges: {0}")]
    InsufficientPrivileges(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};

    const TEST_SECRET: &[u8] = b"test-secret-key-for-pact-development";
    const TEST_ISSUER: &str = "https://auth.test.example.com";
    const TEST_AUDIENCE: &str = "pact-agent";

    fn test_config() -> AuthConfig {
        AuthConfig {
            issuer: TEST_ISSUER.into(),
            audience: TEST_AUDIENCE.into(),
            hmac_secret: Some(TEST_SECRET.to_vec()),
            jwks_url: None,
        }
    }

    fn make_token(claims: &TokenClaims) -> String {
        encode(&Header::default(), claims, &EncodingKey::from_secret(TEST_SECRET)).unwrap()
    }

    fn valid_claims() -> TokenClaims {
        TokenClaims {
            sub: "admin@example.com".into(),
            aud: StringOrVec::Single(TEST_AUDIENCE.into()),
            iss: TEST_ISSUER.into(),
            exp: (chrono::Utc::now().timestamp() + 3600) as u64,
            iat: chrono::Utc::now().timestamp() as u64,
            pact_role: Some("pact-platform-admin".into()),
            pact_principal_type: None,
        }
    }

    #[test]
    fn extract_bearer_token_valid() {
        assert_eq!(extract_bearer_token("Bearer abc123"), Some("abc123"));
        assert_eq!(extract_bearer_token("bearer abc123"), Some("abc123"));
    }

    #[test]
    fn extract_bearer_token_invalid() {
        assert_eq!(extract_bearer_token("Basic abc123"), None);
        assert_eq!(extract_bearer_token("abc123"), None);
        assert_eq!(extract_bearer_token(""), None);
    }

    #[test]
    fn validate_valid_token() {
        let claims = valid_claims();
        let token = make_token(&claims);
        let config = test_config();

        let result = validate_token(&token, &config).unwrap();
        assert_eq!(result.sub, "admin@example.com");
        assert_eq!(result.pact_role, Some("pact-platform-admin".into()));
    }

    #[test]
    fn validate_expired_token() {
        let mut claims = valid_claims();
        claims.exp = (chrono::Utc::now().timestamp() - 3600) as u64; // expired 1h ago
        let token = make_token(&claims);
        let config = test_config();

        let result = validate_token(&token, &config);
        assert!(matches!(result, Err(AuthError::TokenExpired)));
    }

    #[test]
    fn validate_wrong_audience() {
        let mut claims = valid_claims();
        claims.aud = StringOrVec::Single("wrong-audience".into());
        let token = make_token(&claims);
        let config = test_config();

        let result = validate_token(&token, &config);
        assert!(matches!(result, Err(AuthError::InvalidAudience(_))));
    }

    #[test]
    fn validate_wrong_issuer() {
        let mut claims = valid_claims();
        claims.iss = "https://wrong-issuer.com".into();
        let token = make_token(&claims);
        let config = test_config();

        let result = validate_token(&token, &config);
        assert!(matches!(result, Err(AuthError::InvalidIssuer(_))));
    }

    #[test]
    fn validate_no_signing_key() {
        let claims = valid_claims();
        let token = make_token(&claims);
        let config = AuthConfig {
            issuer: TEST_ISSUER.into(),
            audience: TEST_AUDIENCE.into(),
            hmac_secret: None,
            jwks_url: None,
        };

        let result = validate_token(&token, &config);
        assert!(matches!(result, Err(AuthError::NoSigningKey)));
    }

    #[test]
    fn validate_garbage_token() {
        let config = test_config();
        let result = validate_token("not.a.jwt", &config);
        assert!(matches!(result, Err(AuthError::InvalidToken(_))));
    }

    #[test]
    fn claims_to_identity_human() {
        let claims = valid_claims();
        let identity = claims_to_identity(&claims);
        assert_eq!(identity.principal, "admin@example.com");
        assert_eq!(identity.principal_type, PrincipalType::Human);
        assert_eq!(identity.role, "pact-platform-admin");
    }

    #[test]
    fn claims_to_identity_service_agent() {
        let mut claims = valid_claims();
        claims.pact_principal_type = Some("service".into());
        claims.pact_role = Some("pact-service-agent".into());

        let identity = claims_to_identity(&claims);
        assert_eq!(identity.principal_type, PrincipalType::Service);
        assert_eq!(identity.role, "pact-service-agent");
    }

    #[test]
    fn is_platform_admin_check() {
        let admin = Identity {
            principal: "admin@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-platform-admin".into(),
        };
        assert!(is_platform_admin(&admin));

        let ops = Identity {
            principal: "ops@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-ops-ml".into(),
        };
        assert!(!is_platform_admin(&ops));
    }

    #[test]
    fn has_ops_role_check() {
        let admin = Identity {
            principal: "admin@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-platform-admin".into(),
        };
        // Platform admin has ops on any vcluster
        assert!(has_ops_role(&admin, "ml-training"));
        assert!(has_ops_role(&admin, "anything"));

        let ops = Identity {
            principal: "ops@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-ops-ml-training".into(),
        };
        assert!(has_ops_role(&ops, "ml-training"));
        assert!(!has_ops_role(&ops, "other-vcluster"));
    }

    #[test]
    fn has_viewer_role_includes_ops_and_admin() {
        let viewer = Identity {
            principal: "viewer@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-viewer-ml".into(),
        };
        assert!(has_viewer_role(&viewer, "ml"));
        assert!(!has_viewer_role(&viewer, "other"));

        // Ops should also have viewer access
        let ops = Identity {
            principal: "ops@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-ops-ml".into(),
        };
        assert!(has_viewer_role(&ops, "ml"));
    }

    #[test]
    fn string_or_vec_contains() {
        assert!(!StringOrVec::None.contains("test"));

        let single = StringOrVec::Single("pact-agent".into());
        assert!(single.contains("pact-agent"));
        assert!(!single.contains("other"));

        let multi = StringOrVec::Multiple(vec!["pact-agent".into(), "pact-cli".into()]);
        assert!(multi.contains("pact-agent"));
        assert!(multi.contains("pact-cli"));
        assert!(!multi.contains("other"));
    }

    // -----------------------------------------------------------------------
    // JWKS cache tests
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn jwks_cache_starts_empty() {
        let cache = JwksCache::new();
        let keys = cache.cached_keys().await;
        assert!(keys.is_empty());
    }

    #[tokio::test]
    async fn jwks_cache_set_and_get_keys() {
        let cache = JwksCache::new();
        let test_keys = vec![
            Jwk {
                kty: "RSA".into(),
                kid: Some("key-1".into()),
                alg: Some("RS256".into()),
                n: Some("modulus-base64url".into()),
                e: Some("AQAB".into()),
                key_use: Some("sig".into()),
            },
            Jwk {
                kty: "RSA".into(),
                kid: Some("key-2".into()),
                alg: Some("RS256".into()),
                n: Some("other-modulus".into()),
                e: Some("AQAB".into()),
                key_use: Some("sig".into()),
            },
        ];

        cache.set_keys(test_keys.clone()).await;

        let cached = cache.cached_keys().await;
        assert_eq!(cached.len(), 2);
        assert_eq!(cached[0].kid, Some("key-1".into()));
        assert_eq!(cached[1].kid, Some("key-2".into()));
    }

    #[tokio::test]
    async fn jwks_cache_with_custom_ttl() {
        let cache = JwksCache::with_ttl(std::time::Duration::from_secs(60));
        assert!(cache.cached_keys().await.is_empty());

        let keys = vec![Jwk {
            kty: "RSA".into(),
            kid: Some("test".into()),
            alg: None,
            n: Some("n".into()),
            e: Some("e".into()),
            key_use: None,
        }];
        cache.set_keys(keys).await;
        assert_eq!(cache.cached_keys().await.len(), 1);
    }

    #[tokio::test]
    async fn jwks_cache_fetch_fails_without_server() {
        let cache = JwksCache::new();
        // Fetching from a non-existent URL should return an error.
        let result = cache.fetch("http://127.0.0.1:1/jwks.json").await;
        assert!(matches!(result, Err(AuthError::JwksFetchError(_))));
    }

    #[tokio::test]
    async fn jwks_cache_degraded_mode_returns_stale_keys() {
        let cache = JwksCache::with_ttl(std::time::Duration::from_secs(0)); // TTL=0 to force expiry
        let keys = vec![Jwk {
            kty: "RSA".into(),
            kid: Some("stale-key".into()),
            alg: Some("RS256".into()),
            n: Some("n".into()),
            e: Some("e".into()),
            key_use: Some("sig".into()),
        }];
        cache.set_keys(keys).await;

        // TTL is 0 so cache is immediately stale. Fetch will fail but should
        // return stale keys in degraded mode.
        let result = cache.fetch("http://127.0.0.1:1/jwks.json").await;
        assert!(result.is_ok());
        let returned = result.unwrap();
        assert_eq!(returned.len(), 1);
        assert_eq!(returned[0].kid, Some("stale-key".into()));
    }

    #[test]
    fn jwks_response_deserializes() {
        let json = r#"{
            "keys": [
                {
                    "kty": "RSA",
                    "kid": "abc123",
                    "alg": "RS256",
                    "use": "sig",
                    "n": "0vx7agoebGcQSuuPiLJXZptN9nndrQmbXEps2aiAFbWhM78LhWx4cbbfAAtVT86zwu1RK7aPFFxuhDR1L6tSoc_BJECPebWKRXjBZCiFV4n3oknjhMstn64tZ_2W-5JsGY4Hc5n9yBXArwl93lqt7_RN5w6Cf0h4QyQ5v-65YGjQR0_FDW2QvzqY368QQMicAtaSqzs8KJZgnYb9c7d0zgdAZHzu6qMQvRL5hajrn1n91CbOpbISD08qNLyrdkt-bFTWhAI4vMQFh6WeZu0fM4lFd2NcRwr3XPksINHaQ-G_xBniIqbw0Ls1jF44-csFCur-kEgU8awapJzKnqDKgw",
                    "e": "AQAB"
                }
            ]
        }"#;

        let jwks: JwksResponse = serde_json::from_str(json).unwrap();
        assert_eq!(jwks.keys.len(), 1);
        assert_eq!(jwks.keys[0].kty, "RSA");
        assert_eq!(jwks.keys[0].kid, Some("abc123".into()));
        assert_eq!(jwks.keys[0].key_use, Some("sig".into()));
        assert!(jwks.keys[0].n.is_some());
        assert_eq!(jwks.keys[0].e, Some("AQAB".into()));
    }

    #[tokio::test]
    async fn validate_token_with_jwks_falls_back_to_hs256() {
        // When hmac_secret is set, HS256 should work even via the JWKS path.
        let claims = valid_claims();
        let token = make_token(&claims);
        let config = test_config();
        let cache = JwksCache::new();

        let result = validate_token_with_jwks(&token, &config, &cache).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().sub, "admin@example.com");
    }

    #[tokio::test]
    async fn validate_token_with_jwks_no_secret_no_keys_fails() {
        let claims = valid_claims();
        let token = make_token(&claims);
        let config = AuthConfig {
            issuer: TEST_ISSUER.into(),
            audience: TEST_AUDIENCE.into(),
            hmac_secret: None,
            jwks_url: None,
        };
        let cache = JwksCache::new();

        // No HMAC secret and JWKS fetch will fail (no server).
        let result = validate_token_with_jwks(&token, &config, &cache).await;
        assert!(result.is_err());
    }

    #[test]
    fn jwks_url_derivation_from_issuer() {
        let config = AuthConfig {
            issuer: "https://auth.example.com".into(),
            audience: "pact".into(),
            hmac_secret: None,
            jwks_url: None,
        };
        let expected = "https://auth.example.com/.well-known/jwks.json";
        let url = config.jwks_url.clone().unwrap_or_else(|| {
            let issuer = config.issuer.trim_end_matches('/');
            format!("{issuer}/.well-known/jwks.json")
        });
        assert_eq!(url, expected);

        // Trailing slash should be normalized.
        let config2 = AuthConfig {
            issuer: "https://auth.example.com/".into(),
            audience: "pact".into(),
            hmac_secret: None,
            jwks_url: None,
        };
        let url2 = config2.jwks_url.clone().unwrap_or_else(|| {
            let issuer = config2.issuer.trim_end_matches('/');
            format!("{issuer}/.well-known/jwks.json")
        });
        assert_eq!(url2, expected);
    }

    #[test]
    fn jwks_cache_default_trait() {
        let cache = JwksCache::default();
        // Just ensure it constructs without panicking.
        assert_eq!(cache.ttl, std::time::Duration::from_secs(3600));
    }
}
