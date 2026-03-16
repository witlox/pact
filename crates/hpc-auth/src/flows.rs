use std::collections::HashMap;

use chrono::{Duration, Utc};
use serde::Deserialize;

use crate::error::AuthError;
use crate::types::{OidcDiscovery, TokenSet};

/// OAuth2 token response from the IdP.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    scope: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    token_type: String,
}

impl TokenResponse {
    fn into_token_set(self) -> TokenSet {
        let expires_at = Utc::now() + Duration::seconds(self.expires_in.unwrap_or(3600) as i64);
        let scopes = self
            .scope
            .map(|s| s.split_whitespace().map(String::from).collect())
            .unwrap_or_default();
        TokenSet {
            access_token: self.access_token,
            refresh_token: self.refresh_token,
            expires_at,
            scopes,
        }
    }
}

/// Authorization Code with PKCE flow.
///
/// Not yet implemented — requires browser interaction.
pub async fn auth_code_pkce(
    _discovery: &OidcDiscovery,
    _client_id: &str,
    _redirect_port: u16,
) -> Result<TokenSet, AuthError> {
    Err(AuthError::Internal("auth_code_pkce flow not yet implemented".to_string()))
}

/// Device Authorization Grant flow.
///
/// Not yet implemented — requires polling and user interaction.
pub async fn device_code(
    _discovery: &OidcDiscovery,
    _client_id: &str,
) -> Result<TokenSet, AuthError> {
    Err(AuthError::Internal("device_code flow not yet implemented".to_string()))
}

/// Client Credentials flow (machine-to-machine).
///
/// This is the simplest OAuth2 flow — a POST to the token endpoint.
pub async fn client_credentials(
    token_endpoint: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<TokenSet, AuthError> {
    let client = reqwest::Client::new();

    let mut params = HashMap::new();
    params.insert("grant_type", "client_credentials");
    params.insert("client_id", client_id);
    params.insert("client_secret", client_secret);

    let response = client
        .post(token_endpoint)
        .form(&params)
        .send()
        .await
        .map_err(|e| AuthError::IdpUnreachable(format!("{token_endpoint}: {e}")))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AuthError::OAuthFailed(format!(
            "client_credentials failed: HTTP {status}: {body}"
        )));
    }

    let token_response: TokenResponse = response
        .json()
        .await
        .map_err(|e| AuthError::OAuthFailed(format!("invalid token response: {e}")))?;

    Ok(token_response.into_token_set())
}

/// Manual token paste flow (for SSH sessions without browser).
///
/// Not yet implemented — requires stdin interaction.
pub async fn manual_paste(
    _discovery: &OidcDiscovery,
    _client_id: &str,
) -> Result<TokenSet, AuthError> {
    Err(AuthError::Internal("manual_paste flow not yet implemented".to_string()))
}

/// Refresh token flow.
pub async fn refresh_token(
    token_endpoint: &str,
    refresh_tok: &str,
    client_id: &str,
) -> Result<TokenSet, AuthError> {
    let client = reqwest::Client::new();

    let mut params = HashMap::new();
    params.insert("grant_type", "refresh_token");
    params.insert("refresh_token", refresh_tok);
    params.insert("client_id", client_id);

    let response = client
        .post(token_endpoint)
        .form(&params)
        .send()
        .await
        .map_err(|e| AuthError::IdpUnreachable(format!("{token_endpoint}: {e}")))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AuthError::OAuthFailed(format!("refresh_token failed: HTTP {status}: {body}")));
    }

    let token_response: TokenResponse = response
        .json()
        .await
        .map_err(|e| AuthError::OAuthFailed(format!("invalid token response: {e}")))?;

    Ok(token_response.into_token_set())
}
