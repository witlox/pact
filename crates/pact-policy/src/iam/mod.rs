//! OIDC/JWT identity and access management.
//!
//! Token validation flow:
//! 1. Extract Bearer token from gRPC metadata
//! 2. Decode JWT header → find key ID (kid)
//! 3. Validate signature against cached JWKS
//! 4. Check expiry, audience, issuer
//! 5. Extract principal identity (sub, groups, pact_role)
//!
//! Degraded mode (P7): if JWKS refresh fails, use cached keys.

use async_trait::async_trait;
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use tracing::debug;

use pact_common::types::{Identity, PrincipalType};

/// Trait for token validation backends.
#[async_trait]
pub trait TokenValidator: Send + Sync {
    /// Validate a token and return the authenticated identity.
    async fn validate(&self, token: &str) -> Result<Identity, AuthError>;
}

/// JWT claims extracted from an OIDC token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenClaims {
    pub sub: String,
    #[serde(default)]
    pub aud: ClaimAudience,
    #[serde(default)]
    pub iss: String,
    #[serde(default)]
    pub exp: u64,
    #[serde(default)]
    pub iat: u64,
    /// Custom claim: pact role.
    #[serde(default, rename = "pact_role")]
    pub pact_role: Option<String>,
    /// Custom claim: principal type.
    #[serde(default, rename = "pact_principal_type")]
    pub pact_principal_type: Option<String>,
    /// Standard claim: groups (from IdP).
    #[serde(default)]
    pub groups: Vec<String>,
}

/// JWT audience can be string or array.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ClaimAudience {
    #[default]
    None,
    Single(String),
    Multiple(Vec<String>),
}

/// OIDC provider configuration.
#[derive(Debug, Clone)]
pub struct OidcConfig {
    pub issuer: String,
    pub audience: String,
    /// HMAC secret for dev/test. Production uses JWKS (RS256).
    pub hmac_secret: Option<Vec<u8>>,
}

/// HMAC-based token validator for development and testing.
///
/// Production deployments should use a JWKS-based validator.
pub struct HmacTokenValidator {
    config: OidcConfig,
}

impl HmacTokenValidator {
    pub fn new(config: OidcConfig) -> Self {
        Self { config }
    }

    /// Validate token synchronously (HMAC is fast, no I/O needed).
    pub fn validate_sync(&self, token: &str) -> Result<Identity, AuthError> {
        let secret = self.config.hmac_secret.as_deref().ok_or(AuthError::NoSigningKey)?;

        let mut validation = Validation::new(Algorithm::HS256);
        validation.set_audience(&[&self.config.audience]);
        validation.set_issuer(&[&self.config.issuer]);
        validation.validate_exp = true;

        let token_data = decode::<TokenClaims>(
            token,
            &DecodingKey::from_secret(secret),
            &validation,
        )
        .map_err(|e| {
            debug!(error = %e, "Token validation failed");
            match e.kind() {
                jsonwebtoken::errors::ErrorKind::ExpiredSignature => AuthError::TokenExpired,
                jsonwebtoken::errors::ErrorKind::InvalidAudience => {
                    AuthError::InvalidAudience(self.config.audience.clone())
                }
                jsonwebtoken::errors::ErrorKind::InvalidIssuer => {
                    AuthError::InvalidIssuer(self.config.issuer.clone())
                }
                _ => AuthError::InvalidToken(e.to_string()),
            }
        })?;

        Ok(claims_to_identity(&token_data.claims))
    }
}

#[async_trait]
impl TokenValidator for HmacTokenValidator {
    async fn validate(&self, token: &str) -> Result<Identity, AuthError> {
        self.validate_sync(token)
    }
}

/// Convert token claims to a pact Identity.
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

// ---------------------------------------------------------------------------
// JWKS token validator (production)
// ---------------------------------------------------------------------------

/// JWKS response from an OIDC provider's keys endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwksResponse {
    pub keys: Vec<Jwk>,
}

/// A single JSON Web Key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Jwk {
    pub kty: String,
    #[serde(default)]
    pub kid: Option<String>,
    #[serde(default)]
    pub alg: Option<String>,
    #[serde(default)]
    pub n: Option<String>,
    #[serde(default)]
    pub e: Option<String>,
    #[serde(default, rename = "use")]
    pub key_use: Option<String>,
}

/// Cache for JWKS keys fetched from an OIDC provider.
///
/// Keys are cached for a configurable TTL (default 1 hour).
/// In degraded mode (P7), stale cached keys are returned if refresh fails.
#[derive(Debug, Clone)]
pub struct JwksCache {
    inner: std::sync::Arc<tokio::sync::RwLock<JwksCacheInner>>,
    ttl: std::time::Duration,
}

#[derive(Debug)]
struct JwksCacheInner {
    keys: Vec<Jwk>,
    fetched_at: Option<std::time::Instant>,
}

impl JwksCache {
    pub fn new() -> Self {
        Self {
            inner: std::sync::Arc::new(tokio::sync::RwLock::new(JwksCacheInner {
                keys: Vec::new(),
                fetched_at: None,
            })),
            ttl: std::time::Duration::from_secs(3600),
        }
    }

    pub fn with_ttl(ttl: std::time::Duration) -> Self {
        Self {
            inner: std::sync::Arc::new(tokio::sync::RwLock::new(JwksCacheInner {
                keys: Vec::new(),
                fetched_at: None,
            })),
            ttl,
        }
    }

    /// Fetch JWKS keys, using cache if still valid.
    /// Degraded mode (P7): returns stale keys if refresh fails.
    #[cfg(feature = "jwks")]
    pub async fn fetch(&self, jwks_url: &str) -> Result<Vec<Jwk>, AuthError> {
        {
            let inner = self.inner.read().await;
            if let Some(fetched_at) = inner.fetched_at {
                if fetched_at.elapsed() < self.ttl && !inner.keys.is_empty() {
                    return Ok(inner.keys.clone());
                }
            }
        }

        match self.fetch_remote(jwks_url).await {
            Ok(keys) => {
                let mut inner = self.inner.write().await;
                inner.keys.clone_from(&keys);
                inner.fetched_at = Some(std::time::Instant::now());
                Ok(keys)
            }
            Err(e) => {
                let inner = self.inner.read().await;
                if inner.keys.is_empty() {
                    Err(e)
                } else {
                    tracing::warn!(error = %e, "JWKS refresh failed, using stale cached keys (P7 degraded mode)");
                    Ok(inner.keys.clone())
                }
            }
        }
    }

    /// Stub for non-jwks builds: always returns empty.
    #[cfg(not(feature = "jwks"))]
    pub async fn fetch(&self, _jwks_url: &str) -> Result<Vec<Jwk>, AuthError> {
        Err(AuthError::JwksFetchError("JWKS feature not enabled".into()))
    }

    pub async fn cached_keys(&self) -> Vec<Jwk> {
        self.inner.read().await.keys.clone()
    }

    pub async fn set_keys(&self, keys: Vec<Jwk>) {
        let mut inner = self.inner.write().await;
        inner.keys = keys;
        inner.fetched_at = Some(std::time::Instant::now());
    }

    #[cfg(feature = "jwks")]
    async fn fetch_remote(&self, jwks_url: &str) -> Result<Vec<Jwk>, AuthError> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
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

/// Production token validator: tries HMAC (HS256) first, falls back to JWKS (RS256).
///
/// This replaces `HmacTokenValidator` for production use. When `hmac_secret` is set,
/// it tries HS256 first (backward compatible with dev/test tokens). When that fails
/// or no secret is configured, it fetches JWKS keys and validates with RS256.
pub struct JwksTokenValidator {
    config: OidcConfig,
    jwks_cache: JwksCache,
    /// JWKS URL — derived from issuer if not explicitly set.
    jwks_url: String,
}

impl JwksTokenValidator {
    pub fn new(config: OidcConfig, jwks_url: Option<String>) -> Self {
        let url = jwks_url.unwrap_or_else(|| {
            let issuer = config.issuer.trim_end_matches('/');
            format!("{issuer}/.well-known/jwks.json")
        });
        Self { config, jwks_cache: JwksCache::new(), jwks_url: url }
    }

    pub fn with_jwks_cache(mut self, cache: JwksCache) -> Self {
        self.jwks_cache = cache;
        self
    }

    /// Validate token: HS256 first (if secret set), then RS256 via JWKS.
    async fn validate_impl(&self, token: &str) -> Result<Identity, AuthError> {
        // Try HS256 first if HMAC secret is available.
        if self.config.hmac_secret.is_some() {
            let hmac = HmacTokenValidator::new(self.config.clone());
            match hmac.validate_sync(token) {
                Ok(identity) => return Ok(identity),
                Err(e) => {
                    debug!(error = %e, "HS256 validation failed, trying RS256 with JWKS");
                }
            }
        } else {
            debug!("no HMAC secret, going directly to JWKS");
        }

        // RS256 with JWKS.
        debug!(jwks_url = %self.jwks_url, "fetching JWKS keys");
        let keys = match self.jwks_cache.fetch(&self.jwks_url).await {
            Ok(k) => {
                debug!(count = k.len(), "JWKS keys fetched");
                k
            }
            Err(e) => {
                tracing::warn!(error = %e, jwks_url = %self.jwks_url, "JWKS fetch failed");
                return Err(e);
            }
        };
        if keys.is_empty() {
            tracing::warn!(jwks_url = %self.jwks_url, "JWKS returned zero keys");
            return Err(AuthError::NoSigningKey);
        }

        let header = jsonwebtoken::decode_header(token)
            .map_err(|e| AuthError::InvalidToken(e.to_string()))?;
        let token_kid = header.kid.as_deref();

        let matching_keys: Vec<&Jwk> = keys
            .iter()
            .filter(|k| k.kty == "RSA" && k.n.is_some() && k.e.is_some())
            .filter(|k| match (token_kid, k.kid.as_deref()) {
                (Some(tk), Some(kk)) => tk == kk,
                (Some(_), None) => false,
                (None, _) => true,
            })
            .collect();

        if matching_keys.is_empty() {
            return Err(AuthError::InvalidToken(format!(
                "no matching JWKS key for kid {token_kid:?}"
            )));
        }

        let mut last_err = AuthError::NoSigningKey;
        for jwk in matching_keys {
            let n = jwk.n.as_ref().unwrap();
            let e = jwk.e.as_ref().unwrap();

            let decoding_key = match DecodingKey::from_rsa_components(n, e) {
                Ok(k) => k,
                Err(err) => {
                    debug!(error = %err, kid = ?jwk.kid, "Failed to build RSA key");
                    last_err = AuthError::InvalidToken(err.to_string());
                    continue;
                }
            };

            let mut validation = Validation::new(Algorithm::RS256);
            validation.set_audience(&[&self.config.audience]);
            validation.set_issuer(&[&self.config.issuer]);
            validation.validate_exp = true;

            match decode::<TokenClaims>(token, &decoding_key, &validation) {
                Ok(token_data) => return Ok(claims_to_identity(&token_data.claims)),
                Err(e) => {
                    debug!(error = %e, kid = ?jwk.kid, "RS256 validation failed with this key");
                    last_err = match e.kind() {
                        jsonwebtoken::errors::ErrorKind::ExpiredSignature => {
                            AuthError::TokenExpired
                        }
                        jsonwebtoken::errors::ErrorKind::InvalidAudience => {
                            AuthError::InvalidAudience(self.config.audience.clone())
                        }
                        jsonwebtoken::errors::ErrorKind::InvalidIssuer => {
                            AuthError::InvalidIssuer(self.config.issuer.clone())
                        }
                        _ => AuthError::InvalidToken(e.to_string()),
                    };
                }
            }
        }

        Err(last_err)
    }
}

#[async_trait]
impl TokenValidator for JwksTokenValidator {
    async fn validate(&self, token: &str) -> Result<Identity, AuthError> {
        self.validate_impl(token).await
    }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use jsonwebtoken::{encode, EncodingKey, Header};

    const SECRET: &[u8] = b"test-secret-for-pact-policy";
    const ISSUER: &str = "https://auth.test.example.com";
    const AUDIENCE: &str = "pact";

    fn config() -> OidcConfig {
        OidcConfig {
            issuer: ISSUER.into(),
            audience: AUDIENCE.into(),
            hmac_secret: Some(SECRET.to_vec()),
        }
    }

    fn make_token(sub: &str, role: &str, exp_offset: i64) -> String {
        let claims = TokenClaims {
            sub: sub.into(),
            aud: ClaimAudience::Single(AUDIENCE.into()),
            iss: ISSUER.into(),
            exp: (Utc::now().timestamp() + exp_offset) as u64,
            iat: Utc::now().timestamp() as u64,
            pact_role: Some(role.into()),
            pact_principal_type: None,
            groups: vec![],
        };
        encode(&Header::default(), &claims, &EncodingKey::from_secret(SECRET)).unwrap()
    }

    #[tokio::test]
    async fn validate_valid_token() {
        let validator = HmacTokenValidator::new(config());
        let token = make_token("admin@example.com", "pact-platform-admin", 3600);

        let identity = validator.validate(&token).await.unwrap();
        assert_eq!(identity.principal, "admin@example.com");
        assert_eq!(identity.role, "pact-platform-admin");
        assert_eq!(identity.principal_type, PrincipalType::Human);
    }

    #[tokio::test]
    async fn validate_expired_token() {
        let validator = HmacTokenValidator::new(config());
        let token = make_token("admin@example.com", "pact-platform-admin", -3600);

        let result = validator.validate(&token).await;
        assert!(matches!(result, Err(AuthError::TokenExpired)));
    }

    #[tokio::test]
    async fn validate_wrong_audience() {
        let validator = HmacTokenValidator::new(config());
        let claims = TokenClaims {
            sub: "admin@example.com".into(),
            aud: ClaimAudience::Single("wrong-audience".into()),
            iss: ISSUER.into(),
            exp: (Utc::now().timestamp() + 3600) as u64,
            iat: Utc::now().timestamp() as u64,
            pact_role: Some("pact-platform-admin".into()),
            pact_principal_type: None,
            groups: vec![],
        };
        let token = encode(&Header::default(), &claims, &EncodingKey::from_secret(SECRET)).unwrap();

        let result = validator.validate(&token).await;
        assert!(matches!(result, Err(AuthError::InvalidAudience(_))));
    }

    #[tokio::test]
    async fn validate_wrong_issuer() {
        let validator = HmacTokenValidator::new(config());
        let claims = TokenClaims {
            sub: "admin@example.com".into(),
            aud: ClaimAudience::Single(AUDIENCE.into()),
            iss: "https://wrong-issuer.com".into(),
            exp: (Utc::now().timestamp() + 3600) as u64,
            iat: Utc::now().timestamp() as u64,
            pact_role: Some("pact-platform-admin".into()),
            pact_principal_type: None,
            groups: vec![],
        };
        let token = encode(&Header::default(), &claims, &EncodingKey::from_secret(SECRET)).unwrap();

        let result = validator.validate(&token).await;
        assert!(matches!(result, Err(AuthError::InvalidIssuer(_))));
    }

    #[tokio::test]
    async fn validate_no_signing_key() {
        let validator = HmacTokenValidator::new(OidcConfig {
            issuer: ISSUER.into(),
            audience: AUDIENCE.into(),
            hmac_secret: None,
        });
        let token = make_token("admin@example.com", "pact-platform-admin", 3600);

        let result = validator.validate(&token).await;
        assert!(matches!(result, Err(AuthError::NoSigningKey)));
    }

    #[tokio::test]
    async fn validate_garbage_token() {
        let validator = HmacTokenValidator::new(config());
        let result = validator.validate("not.a.jwt").await;
        assert!(matches!(result, Err(AuthError::InvalidToken(_))));
    }

    #[test]
    fn claims_to_identity_human_default() {
        let claims = TokenClaims {
            sub: "alice@example.com".into(),
            pact_role: Some("pact-ops-ml".into()),
            pact_principal_type: None,
            ..Default::default()
        };
        let id = claims_to_identity(&claims);
        assert_eq!(id.principal, "alice@example.com");
        assert_eq!(id.principal_type, PrincipalType::Human);
        assert_eq!(id.role, "pact-ops-ml");
    }

    #[test]
    fn claims_to_identity_service() {
        let claims = TokenClaims {
            sub: "pact-agent-node-001".into(),
            pact_role: Some("pact-service-agent".into()),
            pact_principal_type: Some("service".into()),
            ..Default::default()
        };
        let id = claims_to_identity(&claims);
        assert_eq!(id.principal_type, PrincipalType::Service);
        assert_eq!(id.role, "pact-service-agent");
    }

    #[test]
    fn claims_to_identity_missing_role() {
        let claims = TokenClaims { sub: "bob@example.com".into(), ..Default::default() };
        let id = claims_to_identity(&claims);
        assert_eq!(id.role, ""); // empty when missing
    }
}

impl Default for TokenClaims {
    fn default() -> Self {
        Self {
            sub: String::new(),
            aud: ClaimAudience::None,
            iss: String::new(),
            exp: 0,
            iat: 0,
            pact_role: None,
            pact_principal_type: None,
            groups: vec![],
        }
    }
}
