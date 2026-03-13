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

use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use tracing::debug;

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
}

/// Extract Bearer token from gRPC metadata value.
pub fn extract_bearer_token(metadata_value: &str) -> Option<&str> {
    metadata_value.strip_prefix("Bearer ").or_else(|| metadata_value.strip_prefix("bearer "))
}

/// Validate a JWT token and return the claims.
///
/// In production, this would use JWKS (RS256) from the OIDC discovery endpoint.
/// For development/testing, HMAC (HS256) with a shared secret is supported.
pub fn validate_token(token: &str, config: &AuthConfig) -> Result<TokenClaims, AuthError> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_audience(&[&config.audience]);
    validation.set_issuer(&[&config.issuer]);
    validation.validate_exp = true;

    let secret = config.hmac_secret.as_deref().ok_or(AuthError::NoSigningKey)?;
    let key = DecodingKey::from_secret(secret);

    let token_data = decode::<TokenClaims>(token, &key, &validation).map_err(|e| {
        debug!(error = %e, "Token validation failed");
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
    })?;

    Ok(token_data.claims)
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
}
