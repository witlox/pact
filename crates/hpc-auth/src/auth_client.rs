use chrono::Utc;
use tracing::{debug, info, warn};

use crate::cache::TokenCache;
use crate::discovery::DiscoveryCache;
use crate::error::AuthError;
use crate::flows;
use crate::types::{AuthClientConfig, OAuthFlow, OidcDiscovery, TokenSet};

/// Main authentication client.
///
/// Manages token acquisition, caching, and refresh for a single server URL.
pub struct AuthClient {
    config: AuthClientConfig,
    cache: TokenCache,
    discovery: DiscoveryCache,
}

impl AuthClient {
    /// Create a new auth client with the given configuration.
    pub fn new(config: AuthClientConfig) -> Result<Self, AuthError> {
        let cache = TokenCache::new(
            dirs::config_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(&config.app_name),
            config.permission_mode,
        );
        let discovery = DiscoveryCache::new(config.timeout);
        Ok(Self { config, cache, discovery })
    }

    /// Create an auth client with an externally-constructed cache (for testing).
    pub fn with_cache(config: AuthClientConfig, cache: TokenCache) -> Result<Self, AuthError> {
        let discovery = DiscoveryCache::new(config.timeout);
        Ok(Self { config, cache, discovery })
    }

    /// Initiate login (Auth8: cascading flow selection).
    ///
    /// 1. Check if already logged in (valid token exists).
    /// 2. Discover IdP (server discovery, then OIDC discovery).
    /// 3. Select flow based on discovery capabilities + config overrides.
    /// 4. Execute flow to get `TokenSet`.
    /// 5. Store in cache.
    pub async fn login(&self) -> Result<TokenSet, AuthError> {
        // Check if already logged in with a valid token.
        if let Ok(Some(tokens)) = self.cache.read(&self.config.server_url) {
            if tokens.expires_at > Utc::now() {
                debug!(server = %self.config.server_url, "already logged in");
                return Ok(tokens);
            }
        }

        // If a flow override is set, use that directly.
        if let Some(ref flow) = self.config.flow_override {
            let tokens = self.execute_flow(flow, None, None).await?;
            self.cache.write(&self.config.server_url, &tokens)?;
            info!(server = %self.config.server_url, "login successful");
            return Ok(tokens);
        }

        // Discover IdP configuration.
        let (discovery, client_id) = self.discover_idp().await?;

        // Auth8: Cascading flow selection based on discovery capabilities.
        let flow = self.select_flow(&discovery, &client_id)?;
        let tokens = self.execute_flow(&flow, Some(&discovery), Some(&client_id)).await?;
        self.cache.write(&self.config.server_url, &tokens)?;
        info!(server = %self.config.server_url, "login successful");
        Ok(tokens)
    }

    /// Logout (Auth4: always clears local cache).
    ///
    /// Clears local cache first, then attempts IdP token revocation.
    /// Local cache is always cleared regardless of IdP revocation result.
    pub async fn logout(&self) -> Result<(), AuthError> {
        // Read existing tokens before clearing (for revocation).
        let tokens = self.cache.read(&self.config.server_url)?;

        // Auth4: Always clear local cache first.
        self.cache.delete(&self.config.server_url)?;
        info!(server = %self.config.server_url, "local token cache cleared");

        // Attempt IdP revocation (best-effort).
        if let Some(tokens) = tokens {
            if let Some(ref refresh_token) = tokens.refresh_token {
                if let Err(e) = self.revoke_token(refresh_token).await {
                    warn!(
                        server = %self.config.server_url,
                        error = %e,
                        "IdP token revocation failed (local cache already cleared)"
                    );
                }
            }
        }

        Ok(())
    }

    /// Get a valid access token (Auth1, Auth3).
    ///
    /// 1. Read from cache.
    /// 2. If valid (not expired), return access_token.
    /// 3. If expired + refresh_token valid, call refresh_token flow.
    /// 4. If both expired, return `TokenExpired` error.
    pub async fn get_token(&self) -> Result<String, AuthError> {
        let tokens = self.cache.read(&self.config.server_url)?.ok_or(AuthError::TokenExpired)?;

        // Token still valid.
        if tokens.expires_at > Utc::now() {
            return Ok(tokens.access_token);
        }

        // Try refresh (Auth3).
        if let Some(ref refresh_tok) = tokens.refresh_token {
            debug!(server = %self.config.server_url, "access token expired, attempting refresh");
            match self.try_refresh(refresh_tok).await {
                Ok(new_tokens) => {
                    self.cache.write(&self.config.server_url, &new_tokens)?;
                    return Ok(new_tokens.access_token);
                }
                Err(e) => {
                    debug!(error = %e, "token refresh failed");
                }
            }
        }

        Err(AuthError::TokenExpired)
    }

    /// Check if a valid token exists without refreshing.
    pub fn is_logged_in(&self) -> bool {
        self.cache
            .read(&self.config.server_url)
            .ok()
            .flatten()
            .is_some_and(|t| t.expires_at > Utc::now())
    }

    /// Get the server URL this client targets.
    pub fn server_url(&self) -> &str {
        &self.config.server_url
    }

    /// Discover IdP configuration.
    ///
    /// If `idp_override` is set, uses that directly. Otherwise, fetches the
    /// journal server's `/auth/discovery` endpoint to get the IdP URL and client ID,
    /// then fetches OIDC discovery from the IdP.
    async fn discover_idp(&self) -> Result<(OidcDiscovery, String), AuthError> {
        if let Some(ref idp) = self.config.idp_override {
            // Fetch real OIDC discovery from the issuer URL.
            let discovery = self.discovery.get(&idp.issuer_url).await.or_else(|_| {
                // Fall back to building discovery from the override config.
                Ok(OidcDiscovery {
                    issuer: idp.issuer_url.clone(),
                    authorization_endpoint: idp.authorization_endpoint.clone(),
                    token_endpoint: idp.token_endpoint.clone(),
                    revocation_endpoint: idp.revocation_endpoint.clone(),
                    device_authorization_endpoint: idp.device_authorization_endpoint.clone(),
                    jwks_uri: String::new(),
                    grant_types_supported: Vec::new(),
                    code_challenge_methods_supported: Vec::new(),
                })
            })?;
            return Ok((discovery, idp.client_id.clone()));
        }

        // Fetch from the journal server's auth discovery endpoint.
        flows::server_discovery(&self.config.server_url, self.config.timeout).await
    }

    /// Auth8: Select the best OAuth2 flow based on discovery capabilities.
    #[allow(clippy::unused_self)]
    fn select_flow(
        &self,
        discovery: &OidcDiscovery,
        _client_id: &str,
    ) -> Result<OAuthFlow, AuthError> {
        let grants = &discovery.grant_types_supported;

        // Cascade: Auth Code PKCE > Device Code > Manual Paste.
        if grants.contains(&"authorization_code".to_string())
            && discovery.code_challenge_methods_supported.contains(&"S256".to_string())
        {
            return Ok(OAuthFlow::AuthCodePkce);
        }

        if grants.contains(&"urn:ietf:params:oauth:grant-type:device_code".to_string())
            && discovery.device_authorization_endpoint.is_some()
        {
            return Ok(OAuthFlow::DeviceCode);
        }

        // Fallback: manual paste.
        Ok(OAuthFlow::ManualPaste)
    }

    /// Execute the selected OAuth2 flow.
    async fn execute_flow(
        &self,
        flow: &OAuthFlow,
        discovery: Option<&OidcDiscovery>,
        client_id: Option<&str>,
    ) -> Result<TokenSet, AuthError> {
        let resolve_client_id = || -> &str {
            client_id
                .or_else(|| self.config.idp_override.as_ref().map(|i| i.client_id.as_str()))
                .unwrap_or_default()
        };

        match flow {
            OAuthFlow::AuthCodePkce => {
                let disc = discovery.ok_or_else(|| {
                    AuthError::Internal("discovery required for auth_code_pkce".to_string())
                })?;
                flows::auth_code_pkce(disc, resolve_client_id(), self.config.timeout).await
            }
            OAuthFlow::DeviceCode => {
                let disc = discovery.ok_or_else(|| {
                    AuthError::Internal("discovery required for device_code".to_string())
                })?;
                flows::device_code(disc, resolve_client_id(), self.config.timeout).await
            }
            OAuthFlow::ClientCredentials { client_id, client_secret } => {
                let endpoint = discovery.map(|d| d.token_endpoint.as_str()).unwrap_or_default();
                flows::client_credentials(endpoint, client_id, client_secret).await
            }
            OAuthFlow::ManualPaste => {
                let disc = discovery.ok_or_else(|| {
                    AuthError::Internal("discovery required for manual_paste".to_string())
                })?;
                flows::manual_paste(disc, resolve_client_id(), self.config.timeout).await
            }
        }
    }

    /// Try to refresh the access token.
    async fn try_refresh(&self, refresh_tok: &str) -> Result<TokenSet, AuthError> {
        // We need the token endpoint. Try discovery or IdP override.
        let token_endpoint = if let Some(ref idp) = self.config.idp_override {
            idp.token_endpoint.clone()
        } else {
            return Err(AuthError::Internal(
                "cannot refresh: no token endpoint available".to_string(),
            ));
        };

        let client_id =
            self.config.idp_override.as_ref().map(|i| i.client_id.as_str()).unwrap_or_default();

        flows::refresh_token(&token_endpoint, refresh_tok, client_id).await
    }

    /// Revoke a token at the IdP (best-effort).
    async fn revoke_token(&self, token: &str) -> Result<(), AuthError> {
        let revocation_endpoint =
            self.config.idp_override.as_ref().and_then(|i| i.revocation_endpoint.as_deref());

        let Some(endpoint) = revocation_endpoint else {
            debug!("no revocation endpoint configured, skipping IdP revocation");
            return Ok(());
        };

        let client = reqwest::Client::builder()
            .timeout(self.config.timeout)
            .build()
            .map_err(|e| AuthError::Internal(format!("http client error: {e}")))?;

        let mut params = std::collections::HashMap::new();
        params.insert("token", token);
        params.insert("token_type_hint", "refresh_token");

        client
            .post(endpoint)
            .form(&params)
            .send()
            .await
            .map_err(|e| AuthError::IdpUnreachable(format!("{endpoint}: {e}")))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::PermissionMode;
    use chrono::Duration;
    use tempfile::TempDir;

    fn test_config(server_url: &str) -> AuthClientConfig {
        AuthClientConfig {
            server_url: server_url.to_string(),
            app_name: "pact-test".to_string(),
            permission_mode: PermissionMode::Strict,
            idp_override: None,
            flow_override: None,
            timeout: std::time::Duration::from_secs(5),
        }
    }

    fn valid_tokens() -> TokenSet {
        TokenSet {
            access_token: "valid_access".to_string(),
            refresh_token: Some("valid_refresh".to_string()),
            expires_at: Utc::now() + Duration::hours(1),
            scopes: vec!["pact:admin".to_string()],
        }
    }

    fn expired_tokens_no_refresh() -> TokenSet {
        TokenSet {
            access_token: "expired_access".to_string(),
            refresh_token: None,
            expires_at: Utc::now() - Duration::hours(1),
            scopes: vec!["pact:admin".to_string()],
        }
    }

    #[tokio::test]
    async fn get_token_returns_valid_cached_token_auth1() {
        let tmp = TempDir::new().unwrap();
        let cache = TokenCache::new(tmp.path().to_path_buf(), PermissionMode::Strict);
        let server = "https://test.example.com";
        cache.write(server, &valid_tokens()).unwrap();

        let client = AuthClient::with_cache(test_config(server), cache).unwrap();
        let token = client.get_token().await.unwrap();
        assert_eq!(token, "valid_access");
    }

    #[tokio::test]
    async fn get_token_returns_expired_error_no_refresh() {
        let tmp = TempDir::new().unwrap();
        let cache = TokenCache::new(tmp.path().to_path_buf(), PermissionMode::Strict);
        let server = "https://test.example.com";
        cache.write(server, &expired_tokens_no_refresh()).unwrap();

        let client = AuthClient::with_cache(test_config(server), cache).unwrap();
        let result = client.get_token().await;
        assert!(matches!(result, Err(AuthError::TokenExpired)));
    }

    #[tokio::test]
    async fn get_token_returns_expired_when_no_cache() {
        let tmp = TempDir::new().unwrap();
        let cache = TokenCache::new(tmp.path().to_path_buf(), PermissionMode::Strict);
        let server = "https://test.example.com";

        let client = AuthClient::with_cache(test_config(server), cache).unwrap();
        let result = client.get_token().await;
        assert!(matches!(result, Err(AuthError::TokenExpired)));
    }

    #[test]
    fn is_logged_in_with_valid_token() {
        let tmp = TempDir::new().unwrap();
        let cache = TokenCache::new(tmp.path().to_path_buf(), PermissionMode::Strict);
        let server = "https://test.example.com";
        cache.write(server, &valid_tokens()).unwrap();

        let client = AuthClient::with_cache(test_config(server), cache).unwrap();
        assert!(client.is_logged_in());
    }

    #[test]
    fn is_logged_in_with_expired_token() {
        let tmp = TempDir::new().unwrap();
        let cache = TokenCache::new(tmp.path().to_path_buf(), PermissionMode::Strict);
        let server = "https://test.example.com";
        cache.write(server, &expired_tokens_no_refresh()).unwrap();

        let client = AuthClient::with_cache(test_config(server), cache).unwrap();
        assert!(!client.is_logged_in());
    }

    #[test]
    fn is_logged_in_with_no_cache() {
        let tmp = TempDir::new().unwrap();
        let cache = TokenCache::new(tmp.path().to_path_buf(), PermissionMode::Strict);
        let server = "https://test.example.com";

        let client = AuthClient::with_cache(test_config(server), cache).unwrap();
        assert!(!client.is_logged_in());
    }

    #[test]
    fn server_url_returns_config_url() {
        let tmp = TempDir::new().unwrap();
        let cache = TokenCache::new(tmp.path().to_path_buf(), PermissionMode::Strict);
        let server = "https://test.example.com:9443";

        let client = AuthClient::with_cache(test_config(server), cache).unwrap();
        assert_eq!(client.server_url(), server);
    }

    #[tokio::test]
    async fn logout_clears_cache_auth4() {
        let tmp = TempDir::new().unwrap();
        let cache = TokenCache::new(tmp.path().to_path_buf(), PermissionMode::Strict);
        let server = "https://test.example.com";
        cache.write(server, &valid_tokens()).unwrap();

        let client = AuthClient::with_cache(test_config(server), cache).unwrap();
        assert!(client.is_logged_in());

        client.logout().await.unwrap();
        assert!(!client.is_logged_in());
    }

    #[test]
    fn select_flow_prefers_pkce_auth8() {
        let tmp = TempDir::new().unwrap();
        let cache = TokenCache::new(tmp.path().to_path_buf(), PermissionMode::Strict);
        let server = "https://test.example.com";
        let client = AuthClient::with_cache(test_config(server), cache).unwrap();

        let discovery = OidcDiscovery {
            issuer: "https://idp.example.com".to_string(),
            authorization_endpoint: "https://idp.example.com/auth".to_string(),
            token_endpoint: "https://idp.example.com/token".to_string(),
            revocation_endpoint: None,
            device_authorization_endpoint: Some("https://idp.example.com/device".to_string()),
            jwks_uri: "https://idp.example.com/certs".to_string(),
            grant_types_supported: vec![
                "authorization_code".to_string(),
                "urn:ietf:params:oauth:grant-type:device_code".to_string(),
            ],
            code_challenge_methods_supported: vec!["S256".to_string()],
        };

        let flow = client.select_flow(&discovery, "client-id").unwrap();
        assert!(matches!(flow, OAuthFlow::AuthCodePkce));
    }

    #[test]
    fn select_flow_falls_back_to_device_code_auth8() {
        let tmp = TempDir::new().unwrap();
        let cache = TokenCache::new(tmp.path().to_path_buf(), PermissionMode::Strict);
        let server = "https://test.example.com";
        let client = AuthClient::with_cache(test_config(server), cache).unwrap();

        let discovery = OidcDiscovery {
            issuer: "https://idp.example.com".to_string(),
            authorization_endpoint: "https://idp.example.com/auth".to_string(),
            token_endpoint: "https://idp.example.com/token".to_string(),
            revocation_endpoint: None,
            device_authorization_endpoint: Some("https://idp.example.com/device".to_string()),
            jwks_uri: "https://idp.example.com/certs".to_string(),
            grant_types_supported: vec!["urn:ietf:params:oauth:grant-type:device_code".to_string()],
            code_challenge_methods_supported: vec![],
        };

        let flow = client.select_flow(&discovery, "client-id").unwrap();
        assert!(matches!(flow, OAuthFlow::DeviceCode));
    }

    #[test]
    fn select_flow_falls_back_to_manual_paste_auth8() {
        let tmp = TempDir::new().unwrap();
        let cache = TokenCache::new(tmp.path().to_path_buf(), PermissionMode::Strict);
        let server = "https://test.example.com";
        let client = AuthClient::with_cache(test_config(server), cache).unwrap();

        let discovery = OidcDiscovery {
            issuer: "https://idp.example.com".to_string(),
            authorization_endpoint: "https://idp.example.com/auth".to_string(),
            token_endpoint: "https://idp.example.com/token".to_string(),
            revocation_endpoint: None,
            device_authorization_endpoint: None,
            jwks_uri: "https://idp.example.com/certs".to_string(),
            grant_types_supported: vec!["refresh_token".to_string()],
            code_challenge_methods_supported: vec![],
        };

        let flow = client.select_flow(&discovery, "client-id").unwrap();
        assert!(matches!(flow, OAuthFlow::ManualPaste));
    }
}
