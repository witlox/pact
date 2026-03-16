use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::error::AuthError;
use crate::types::{PermissionMode, TokenSet};

/// On-disk token cache file format.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TokenCacheFile {
    version: u32,
    #[serde(default)]
    default_server: Option<String>,
    #[serde(default)]
    servers: HashMap<String, CachedTokenEntry>,
}

/// Per-server entry in the cache file.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedTokenEntry {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: DateTime<Utc>,
    scopes: Vec<String>,
    #[serde(default)]
    idp_issuer: Option<String>,
}

impl From<CachedTokenEntry> for TokenSet {
    fn from(entry: CachedTokenEntry) -> Self {
        Self {
            access_token: entry.access_token,
            refresh_token: entry.refresh_token,
            expires_at: entry.expires_at,
            scopes: entry.scopes,
        }
    }
}

impl From<&TokenSet> for CachedTokenEntry {
    fn from(tokens: &TokenSet) -> Self {
        Self {
            access_token: tokens.access_token.clone(),
            refresh_token: tokens.refresh_token.clone(),
            expires_at: tokens.expires_at,
            scopes: tokens.scopes.clone(),
            idp_issuer: None,
        }
    }
}

/// File-based token cache, keyed by server URL (Auth6).
pub struct TokenCache {
    cache_dir: PathBuf,
    permission_mode: PermissionMode,
}

impl TokenCache {
    /// Create a new token cache.
    ///
    /// `cache_dir` is the application config directory (e.g. `~/.config/pact`).
    pub fn new(cache_dir: PathBuf, permission_mode: PermissionMode) -> Self {
        Self { cache_dir, permission_mode }
    }

    /// Create a token cache using the default config directory for the given app name.
    pub fn default_for_app(app_name: &str, permission_mode: PermissionMode) -> Self {
        let cache_dir = dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join(app_name);
        Self::new(cache_dir, permission_mode)
    }

    fn cache_path(&self) -> PathBuf {
        self.cache_dir.join("tokens.json")
    }

    fn read_file(&self) -> Result<TokenCacheFile, AuthError> {
        let path = self.cache_path();
        if !path.exists() {
            return Ok(TokenCacheFile {
                version: 1,
                default_server: None,
                servers: HashMap::new(),
            });
        }

        // Auth5: Validate file permissions.
        self.check_permissions(&path)?;

        let contents = fs::read_to_string(&path).map_err(|e| {
            AuthError::CacheCorrupted(format!("cannot read {}: {e}", path.display()))
        })?;

        // Auth2: Fail closed on corruption.
        serde_json::from_str::<TokenCacheFile>(&contents).map_err(|e| {
            AuthError::CacheCorrupted(format!("invalid JSON in {}: {e}", path.display()))
        })
    }

    fn write_file(&self, cache: &TokenCacheFile) -> Result<(), AuthError> {
        // Ensure directory exists.
        if let Some(parent) = self.cache_path().parent() {
            fs::create_dir_all(parent).map_err(|e| {
                AuthError::Internal(format!(
                    "cannot create cache directory {}: {e}",
                    parent.display()
                ))
            })?;
        }

        let path = self.cache_path();
        let contents = serde_json::to_string_pretty(cache)
            .map_err(|e| AuthError::Internal(format!("cannot serialize token cache: {e}")))?;

        fs::write(&path, contents)
            .map_err(|e| AuthError::Internal(format!("cannot write {}: {e}", path.display())))?;

        // Set 0600 permissions on Unix.
        Self::set_restrictive_permissions(&path)?;

        Ok(())
    }

    /// Check file permissions (Auth5).
    fn check_permissions(&self, path: &Path) -> Result<(), AuthError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = fs::metadata(path).map_err(|e| {
                AuthError::CacheCorrupted(format!(
                    "cannot read metadata for {}: {e}",
                    path.display()
                ))
            })?;
            let mode = metadata.permissions().mode() & 0o777;
            if mode != 0o600 {
                match self.permission_mode {
                    PermissionMode::Strict => {
                        return Err(AuthError::CachePermissionDenied(format!(
                            "token cache {} has permissions {mode:04o}, expected 0600",
                            path.display()
                        )));
                    }
                    PermissionMode::Lenient => {
                        warn!(
                            "token cache {} has permissions {:04o}, fixing to 0600",
                            path.display(),
                            mode
                        );
                        Self::set_restrictive_permissions(path)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Set file to 0600 on Unix.
    fn set_restrictive_permissions(path: &Path) -> Result<(), AuthError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::Permissions::from_mode(0o600);
            fs::set_permissions(path, perms).map_err(|e| {
                AuthError::Internal(format!("cannot set permissions on {}: {e}", path.display()))
            })?;
        }
        Ok(())
    }

    /// Read tokens for a specific server (Auth6: per-server isolation).
    pub fn read(&self, server_url: &str) -> Result<Option<TokenSet>, AuthError> {
        let cache = self.read_file()?;
        Ok(cache.servers.get(server_url).cloned().map(TokenSet::from))
    }

    /// Write tokens for a specific server (Auth5: 0600 permissions).
    pub fn write(&self, server_url: &str, tokens: &TokenSet) -> Result<(), AuthError> {
        let mut cache = self.read_file()?;
        cache.servers.insert(server_url.to_string(), tokens.into());
        self.write_file(&cache)
    }

    /// Delete tokens for a specific server.
    pub fn delete(&self, server_url: &str) -> Result<(), AuthError> {
        let mut cache = self.read_file()?;
        cache.servers.remove(server_url);
        // If the default server is the one being deleted, clear it.
        if cache.default_server.as_deref() == Some(server_url) {
            cache.default_server = None;
        }
        self.write_file(&cache)
    }

    /// List servers with cached tokens.
    pub fn list_servers(&self) -> Vec<String> {
        self.read_file().map(|c| c.servers.keys().cloned().collect()).unwrap_or_default()
    }

    /// Get the default server URL.
    pub fn default_server(&self) -> Option<String> {
        self.read_file().ok().and_then(|c| c.default_server)
    }

    /// Set the default server URL.
    pub fn set_default_server(&self, server_url: &str) -> Result<(), AuthError> {
        let mut cache = self.read_file()?;
        cache.default_server = Some(server_url.to_string());
        self.write_file(&cache)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use tempfile::TempDir;

    fn test_cache(dir: &Path) -> TokenCache {
        TokenCache::new(dir.to_path_buf(), PermissionMode::Strict)
    }

    fn test_tokens(suffix: &str) -> TokenSet {
        TokenSet {
            access_token: format!("access_{suffix}"),
            refresh_token: Some(format!("refresh_{suffix}")),
            expires_at: Utc::now() + chrono::Duration::hours(1),
            scopes: vec!["pact:admin".to_string()],
        }
    }

    #[test]
    fn read_empty_cache_returns_none() {
        let tmp = TempDir::new().unwrap();
        let cache = test_cache(tmp.path());
        let result = cache.read("https://example.com").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn write_then_read_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let cache = test_cache(tmp.path());
        let tokens = test_tokens("a");
        cache.write("https://example.com", &tokens).unwrap();

        let loaded = cache.read("https://example.com").unwrap().unwrap();
        assert_eq!(loaded.access_token, "access_a");
        assert_eq!(loaded.refresh_token.as_deref(), Some("refresh_a"));
    }

    #[test]
    fn per_server_isolation_auth6() {
        let tmp = TempDir::new().unwrap();
        let cache = test_cache(tmp.path());
        cache.write("https://server-a.com", &test_tokens("a")).unwrap();
        cache.write("https://server-b.com", &test_tokens("b")).unwrap();

        let a = cache.read("https://server-a.com").unwrap().unwrap();
        let b = cache.read("https://server-b.com").unwrap().unwrap();
        assert_eq!(a.access_token, "access_a");
        assert_eq!(b.access_token, "access_b");
    }

    #[test]
    fn delete_removes_server_entry() {
        let tmp = TempDir::new().unwrap();
        let cache = test_cache(tmp.path());
        cache.write("https://example.com", &test_tokens("a")).unwrap();
        cache.delete("https://example.com").unwrap();

        assert!(cache.read("https://example.com").unwrap().is_none());
    }

    #[test]
    fn delete_clears_default_if_matching() {
        let tmp = TempDir::new().unwrap();
        let cache = test_cache(tmp.path());
        cache.write("https://example.com", &test_tokens("a")).unwrap();
        cache.set_default_server("https://example.com").unwrap();
        cache.delete("https://example.com").unwrap();

        assert!(cache.default_server().is_none());
    }

    #[test]
    fn list_servers_returns_all_keys() {
        let tmp = TempDir::new().unwrap();
        let cache = test_cache(tmp.path());
        cache.write("https://a.com", &test_tokens("a")).unwrap();
        cache.write("https://b.com", &test_tokens("b")).unwrap();

        let mut servers = cache.list_servers();
        servers.sort();
        assert_eq!(servers, vec!["https://a.com", "https://b.com"]);
    }

    #[test]
    fn default_server_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let cache = test_cache(tmp.path());
        assert!(cache.default_server().is_none());

        cache.set_default_server("https://example.com").unwrap();
        assert_eq!(cache.default_server().as_deref(), Some("https://example.com"));
    }

    #[test]
    fn corrupted_cache_returns_error_auth2() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("tokens.json");
        fs::write(&path, "not valid json!!!").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }

        let cache = test_cache(tmp.path());
        let result = cache.read("https://example.com");
        assert!(matches!(result, Err(AuthError::CacheCorrupted(_))));
    }

    #[cfg(unix)]
    #[test]
    fn strict_mode_rejects_wrong_permissions_auth5() {
        let tmp = TempDir::new().unwrap();
        let cache = test_cache(tmp.path());
        cache.write("https://example.com", &test_tokens("a")).unwrap();

        // Set wrong permissions.
        use std::os::unix::fs::PermissionsExt;
        let path = cache.cache_path();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        let result = cache.read("https://example.com");
        assert!(matches!(result, Err(AuthError::CachePermissionDenied(_))));
    }

    #[cfg(unix)]
    #[test]
    fn lenient_mode_fixes_permissions_auth5() {
        let tmp = TempDir::new().unwrap();
        let cache = TokenCache::new(tmp.path().to_path_buf(), PermissionMode::Lenient);
        cache.write("https://example.com", &test_tokens("a")).unwrap();

        // Set wrong permissions.
        use std::os::unix::fs::PermissionsExt;
        let path = cache.cache_path();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        // Should succeed (lenient fixes it).
        let result = cache.read("https://example.com").unwrap().unwrap();
        assert_eq!(result.access_token, "access_a");

        // Permissions should now be fixed.
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn write_sets_0600_permissions() {
        let tmp = TempDir::new().unwrap();
        let cache = test_cache(tmp.path());
        cache.write("https://example.com", &test_tokens("a")).unwrap();

        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(cache.cache_path()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn token_debug_redacts_secrets_auth7() {
        let tokens = test_tokens("secret");
        let debug = format!("{tokens:?}");
        assert!(!debug.contains("access_secret"));
        assert!(!debug.contains("refresh_secret"));
        assert!(debug.contains("[redacted]"));
    }
}
