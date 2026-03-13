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
