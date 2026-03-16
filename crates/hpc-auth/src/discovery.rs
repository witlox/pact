use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use chrono::Utc;
use tracing::{debug, warn};

use crate::error::AuthError;
use crate::types::{CachedDiscovery, OidcDiscovery};

/// Default TTL for cached discovery documents (1 hour).
const DEFAULT_TTL_SECS: u64 = 3600;

/// In-memory cache for OIDC discovery documents.
pub struct DiscoveryCache {
    cache: Mutex<HashMap<String, CachedDiscovery>>,
    http_client: reqwest::Client,
}

impl DiscoveryCache {
    /// Create a new discovery cache with the given HTTP request timeout.
    pub fn new(timeout: Duration) -> Self {
        let http_client = reqwest::Client::builder().timeout(timeout).build().unwrap_or_default();
        Self { cache: Mutex::new(HashMap::new()), http_client }
    }

    /// Fetch discovery document for an issuer.
    ///
    /// Returns cached document if fresh. Fetches from
    /// `{issuer}/.well-known/openid-configuration` if stale or missing.
    /// On fetch failure, returns stale cached document (degraded mode).
    pub async fn get(&self, issuer_url: &str) -> Result<OidcDiscovery, AuthError> {
        // Check cache first.
        if let Some(cached) = self.get_cached(issuer_url) {
            let age = Utc::now().signed_duration_since(cached.fetched_at).num_seconds();
            if age >= 0 && (age as u64) < cached.ttl_seconds {
                debug!(issuer = issuer_url, "using cached discovery document");
                return Ok(cached.document);
            }
            // Stale — try to refresh, fall back to stale on failure.
            debug!(issuer = issuer_url, "discovery cache stale, refreshing");
            match self.fetch_discovery(issuer_url).await {
                Ok(doc) => {
                    self.store(issuer_url, doc.clone());
                    return Ok(doc);
                }
                Err(e) => {
                    warn!(
                        issuer = issuer_url,
                        error = %e,
                        "failed to refresh discovery, using stale cache"
                    );
                    return Ok(cached.document);
                }
            }
        }

        // No cache — must fetch.
        let doc = self.fetch_discovery(issuer_url).await?;
        self.store(issuer_url, doc.clone());
        Ok(doc)
    }

    /// Clear cached discovery document (on auth failure suggesting staleness).
    pub fn clear(&self, issuer_url: &str) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.remove(issuer_url);
        }
    }

    fn get_cached(&self, issuer_url: &str) -> Option<CachedDiscovery> {
        self.cache.lock().ok().and_then(|c| c.get(issuer_url).cloned())
    }

    fn store(&self, issuer_url: &str, doc: OidcDiscovery) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.insert(
                issuer_url.to_string(),
                CachedDiscovery {
                    fetched_at: Utc::now(),
                    ttl_seconds: DEFAULT_TTL_SECS,
                    document: doc,
                },
            );
        }
    }

    async fn fetch_discovery(&self, issuer_url: &str) -> Result<OidcDiscovery, AuthError> {
        let url = format!("{}/.well-known/openid-configuration", issuer_url.trim_end_matches('/'));

        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|e| AuthError::IdpUnreachable(format!("{url}: {e}")))?;

        if !response.status().is_success() {
            return Err(AuthError::IdpUnreachable(format!("{url}: HTTP {}", response.status())));
        }

        response
            .json::<OidcDiscovery>()
            .await
            .map_err(|e| AuthError::IdpUnreachable(format!("invalid discovery document: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_discovery() -> OidcDiscovery {
        OidcDiscovery {
            issuer: "https://idp.example.com".to_string(),
            authorization_endpoint: "https://idp.example.com/auth".to_string(),
            token_endpoint: "https://idp.example.com/token".to_string(),
            revocation_endpoint: Some("https://idp.example.com/revoke".to_string()),
            device_authorization_endpoint: None,
            jwks_uri: "https://idp.example.com/certs".to_string(),
            grant_types_supported: vec!["authorization_code".to_string()],
            code_challenge_methods_supported: vec!["S256".to_string()],
        }
    }

    #[test]
    fn cache_stores_and_retrieves() {
        let cache = DiscoveryCache::new(Duration::from_secs(10));
        let doc = test_discovery();
        cache.store("https://idp.example.com", doc.clone());

        let cached = cache.get_cached("https://idp.example.com").unwrap();
        assert_eq!(cached.document.issuer, doc.issuer);
        assert_eq!(cached.ttl_seconds, DEFAULT_TTL_SECS);
    }

    #[test]
    fn clear_removes_entry() {
        let cache = DiscoveryCache::new(Duration::from_secs(10));
        cache.store("https://idp.example.com", test_discovery());
        cache.clear("https://idp.example.com");

        assert!(cache.get_cached("https://idp.example.com").is_none());
    }

    #[test]
    fn clear_nonexistent_is_noop() {
        let cache = DiscoveryCache::new(Duration::from_secs(10));
        cache.clear("https://nonexistent.example.com");
        // No panic.
    }

    #[tokio::test]
    async fn fetch_unreachable_returns_error() {
        let cache = DiscoveryCache::new(Duration::from_secs(1));
        let result = cache.get("https://nonexistent.invalid.test").await;
        assert!(matches!(result, Err(AuthError::IdpUnreachable(_))));
    }

    #[tokio::test]
    async fn stale_cache_returned_on_fetch_failure() {
        let cache = DiscoveryCache::new(Duration::from_secs(1));
        // Insert a stale entry (TTL 0).
        if let Ok(mut c) = cache.cache.lock() {
            c.insert(
                "https://nonexistent.invalid.test".to_string(),
                CachedDiscovery {
                    fetched_at: Utc::now() - chrono::Duration::hours(2),
                    ttl_seconds: 0,
                    document: test_discovery(),
                },
            );
        }

        // Should return the stale document since fetch will fail.
        let result = cache.get("https://nonexistent.invalid.test").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().issuer, "https://idp.example.com");
    }
}
