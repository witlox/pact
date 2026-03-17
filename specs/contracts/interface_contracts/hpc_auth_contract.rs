//! Contract tests for hpc-auth crate interfaces.
//!
//! These tests verify the public surface of the hpc-auth library
//! consumed by:
//! - pact-cli (AuthClient for login/logout/get_token)
//! - lattice-cli (AuthClient with Lenient permission mode)
//!
//! All tests reference their source contract and spec requirement.
//! Tests use stubs — they define WHAT the interface must do,
//! not HOW it does it internally.

// ---------------------------------------------------------------------------
// AuthClient contracts
// ---------------------------------------------------------------------------

/// Contract: hpc-auth.md § AuthClient::login
/// Spec: Auth5 — cache created with 0600 permissions
/// If this test didn't exist: login could store tokens world-readable,
/// leaking credentials to other users on the system.
#[test]
fn login_stores_tokens_with_correct_permissions() {
    let client = stub_auth_client();

    let token_set = client.login().unwrap();

    let cache = stub_token_cache();
    let perms = cache.file_permissions(client.server_url());
    assert_eq!(perms, 0o600);
    assert!(cache.read(client.server_url()).unwrap().is_some());
}

/// Contract: hpc-auth.md § AuthClient::login
/// Spec: Auth8 — cascading flow: PKCE > Confidential > DeviceCode > ManualPaste
/// If this test didn't exist: login might pick a weaker flow when a stronger
/// one is available, or fail when a fallback exists.
#[test]
fn login_selects_flow_per_cascade() {
    // Discovery doc supports all flows
    let client = stub_auth_client();
    assert_eq!(client.selected_flow(), OAuthFlow::AuthCodePkce);

    // Discovery doc lacks PKCE support
    let client = stub_auth_client_without_pkce();
    assert_eq!(client.selected_flow(), OAuthFlow::ClientCredentials);

    // Discovery doc lacks PKCE + no client credentials configured
    let client = stub_auth_client_device_only();
    assert_eq!(client.selected_flow(), OAuthFlow::DeviceCode);

    // Nothing supported → ManualPaste as last resort
    let client = stub_auth_client_no_flows();
    assert_eq!(client.selected_flow(), OAuthFlow::ManualPaste);
}

/// Contract: hpc-auth.md § AuthClient::logout
/// Spec: Auth4 — cache deleted before IdP revocation call
/// If this test didn't exist: a crash during IdP revocation would leave
/// valid tokens on disk, defeating the purpose of logout.
#[test]
fn logout_clears_cache_before_idp_revocation() {
    let client = stub_auth_client();
    client.login().unwrap();

    // Capture ordering: cache delete must happen before IdP call
    let events = client.logout_with_event_log().unwrap();
    let cache_clear_idx = events.iter().position(|e| e == "cache_delete").unwrap();
    let idp_revoke_idx = events.iter().position(|e| e == "idp_revoke").unwrap();
    assert!(cache_clear_idx < idp_revoke_idx);
}

/// Contract: hpc-auth.md § AuthClient::logout
/// Spec: Auth4 — IdP failure does not block local cache clear
/// If this test didn't exist: an unreachable IdP would leave tokens on disk,
/// preventing logout when the network is down.
#[test]
fn logout_clears_cache_even_on_idp_failure() {
    let client = stub_auth_client();
    client.login().unwrap();
    client.set_idp_unreachable(true);

    // Logout may return an error for the IdP call, but cache must be cleared
    let _ = client.logout();

    let cache = stub_token_cache();
    assert!(cache.read(client.server_url()).unwrap().is_none());
}

/// Contract: hpc-auth.md § AuthClient::get_token
/// Spec: Auth3 — returns cached token when not expired
/// If this test didn't exist: every get_token call might hit the IdP,
/// adding latency to every authenticated request.
#[test]
fn get_token_returns_cached_if_valid() {
    let client = stub_auth_client();
    let original = client.login().unwrap();

    let token = client.get_token().unwrap();
    assert_eq!(token, original.access_token);
    assert_eq!(client.idp_request_count(), 1); // Only the initial login
}

/// Contract: hpc-auth.md § AuthClient::get_token
/// Spec: Auth3 — refreshes silently if access token expired but refresh token valid
/// If this test didn't exist: users would be forced to re-login every time
/// the short-lived access token expires.
#[test]
fn get_token_refreshes_silently_if_expired() {
    let client = stub_auth_client();
    client.login().unwrap();
    client.expire_access_token();

    let token = client.get_token().unwrap();
    assert!(!token.is_empty());
    // Should have done a silent refresh, not a full login
    assert_eq!(client.refresh_count(), 1);
}

/// Contract: hpc-auth.md § AuthClient::get_token
/// Spec: Auth1 — error when access + refresh tokens both expired
/// If this test didn't exist: an expired session could silently produce
/// an invalid token, leading to confusing server-side rejections.
#[test]
fn get_token_returns_error_if_both_expired() {
    let client = stub_auth_client();
    client.login().unwrap();
    client.expire_access_token();
    client.expire_refresh_token();

    let result = client.get_token();
    assert_matches!(result, Err(AuthError::TokenExpired));
}

// ---------------------------------------------------------------------------
// TokenCache contracts
// ---------------------------------------------------------------------------

/// Contract: hpc-auth.md § TokenCache::read
/// Spec: Auth5 — strict mode rejects cache with wrong permissions
/// If this test didn't exist: another process could chmod the cache file
/// and hpc-auth would silently read potentially tampered tokens.
#[test]
fn read_validates_permissions_strict() {
    let cache = stub_token_cache();
    cache.write("https://journal.example.com", &test_token_set()).unwrap();
    cache.force_permissions("https://journal.example.com", 0o644);

    let result = cache.read_with_mode("https://journal.example.com", PermissionMode::Strict);
    assert_matches!(result, Err(AuthError::CachePermissionDenied(_)));
}

/// Contract: hpc-auth.md § TokenCache::read
/// Spec: Auth5 — lenient mode warns and fixes wrong permissions
/// If this test didn't exist: lattice-cli (lenient mode) would either
/// reject valid caches or silently ignore permission issues.
#[test]
fn read_validates_permissions_lenient() {
    let cache = stub_token_cache();
    cache.write("https://journal.example.com", &test_token_set()).unwrap();
    cache.force_permissions("https://journal.example.com", 0o644);

    let result = cache.read_with_mode("https://journal.example.com", PermissionMode::Lenient);
    assert!(result.is_ok());
    // Permissions should be fixed to 0600 after lenient read
    assert_eq!(cache.file_permissions("https://journal.example.com"), 0o600);
}

/// Contract: hpc-auth.md § TokenCache::read
/// Spec: Auth2 — fail closed on cache corruption
/// If this test didn't exist: corrupted JSON could cause panics or partial
/// deserialization producing invalid tokens.
#[test]
fn read_rejects_corrupted_json() {
    let cache = stub_token_cache();
    cache.write_raw("https://journal.example.com", b"{ not valid json !!!");

    let result = cache.read("https://journal.example.com");
    assert_matches!(result, Err(AuthError::CacheCorrupted(_)));
}

/// Contract: hpc-auth.md § TokenCache::write
/// Spec: Auth5 — write always creates file with 0600 permissions
/// If this test didn't exist: token files could inherit umask permissions,
/// potentially world-readable on misconfigured systems.
#[test]
fn write_creates_file_with_0600() {
    let cache = stub_token_cache();

    cache.write("https://journal.example.com", &test_token_set()).unwrap();

    assert_eq!(cache.file_permissions("https://journal.example.com"), 0o600);
}

/// Contract: hpc-auth.md § TokenCache
/// Spec: Auth6 — tokens keyed by server URL, no cross-contamination
/// If this test didn't exist: logging into server A could return tokens
/// for server B, causing authentication against the wrong cluster.
#[test]
fn per_server_isolation() {
    let cache = stub_token_cache();

    let tokens_a = test_token_set();
    let tokens_b = TokenSet {
        access_token: "token-for-server-b".into(),
        ..test_token_set()
    };

    cache.write("https://journal-a.example.com", &tokens_a).unwrap();
    cache.write("https://journal-b.example.com", &tokens_b).unwrap();

    let read_a = cache.read("https://journal-a.example.com").unwrap().unwrap();
    let read_b = cache.read("https://journal-b.example.com").unwrap().unwrap();

    assert_eq!(read_a.access_token, tokens_a.access_token);
    assert_eq!(read_b.access_token, tokens_b.access_token);
    assert_ne!(read_a.access_token, read_b.access_token);
}

/// Contract: hpc-auth.md § TokenCache::list_servers
/// Spec: lists all server URLs with cached tokens
/// If this test didn't exist: CLI `auth status` couldn't show which
/// clusters the user is logged into.
#[test]
fn list_servers_returns_all_cached() {
    let cache = stub_token_cache();
    cache.write("https://journal-a.example.com", &test_token_set()).unwrap();
    cache.write("https://journal-b.example.com", &test_token_set()).unwrap();
    cache.write("https://journal-c.example.com", &test_token_set()).unwrap();

    let mut servers = cache.list_servers();
    servers.sort();

    assert_eq!(servers, vec![
        "https://journal-a.example.com",
        "https://journal-b.example.com",
        "https://journal-c.example.com",
    ]);
}

/// Contract: hpc-auth.md § TokenCache::default_server / set_default_server
/// Spec: get/set default server round-trips correctly
/// If this test didn't exist: setting a default server might not persist,
/// forcing users to specify --server on every command.
#[test]
fn default_server_roundtrip() {
    let cache = stub_token_cache();
    assert_eq!(cache.default_server(), None);

    cache.set_default_server("https://journal.example.com").unwrap();
    assert_eq!(cache.default_server(), Some("https://journal.example.com".into()));
}

// ---------------------------------------------------------------------------
// DiscoveryCache contracts
// ---------------------------------------------------------------------------

/// Contract: hpc-auth.md § DiscoveryCache::get
/// Spec: returns cached discovery document when not stale
/// If this test didn't exist: every get_token or login would re-fetch
/// the discovery document, adding an extra HTTP round-trip.
#[test]
fn get_returns_cached_if_fresh() {
    let discovery = stub_discovery_cache();
    let issuer = "https://idp.example.com";

    // First fetch populates cache
    let _ = discovery.get(issuer).unwrap();
    let fetch_count_before = discovery.fetch_count();

    // Second fetch should use cache
    let _ = discovery.get(issuer).unwrap();
    assert_eq!(discovery.fetch_count(), fetch_count_before);
}

/// Contract: hpc-auth.md § DiscoveryCache::get
/// Spec: fetches from .well-known on stale or missing
/// If this test didn't exist: stale discovery docs could cause auth
/// failures if the IdP rotated its endpoints.
#[test]
fn get_fetches_on_stale_or_missing() {
    let discovery = stub_discovery_cache();
    let issuer = "https://idp.example.com";

    // Missing: should fetch
    let doc = discovery.get(issuer).unwrap();
    assert_eq!(discovery.fetch_count(), 1);
    assert_eq!(doc.issuer, issuer);

    // Force stale
    discovery.mark_stale(issuer);

    // Should re-fetch
    let _ = discovery.get(issuer).unwrap();
    assert_eq!(discovery.fetch_count(), 2);
}

/// Contract: hpc-auth.md § DiscoveryCache::get
/// Spec: degraded mode — returns stale document on fetch failure
/// If this test didn't exist: a transient IdP outage would prevent all
/// CLI operations even when cached tokens are still valid.
#[test]
fn get_returns_stale_on_fetch_failure() {
    let discovery = stub_discovery_cache();
    let issuer = "https://idp.example.com";

    // Populate cache
    let _ = discovery.get(issuer).unwrap();

    // Mark stale + make fetch fail
    discovery.mark_stale(issuer);
    discovery.set_fetch_failing(true);

    // Should return stale doc rather than error
    let doc = discovery.get(issuer).unwrap();
    assert_eq!(doc.issuer, issuer);
}

/// Contract: hpc-auth.md § DiscoveryCache::clear
/// Spec: clearing forces next get to fetch fresh
/// If this test didn't exist: an auth failure caused by stale discovery
/// could not be resolved without restarting the CLI.
#[test]
fn clear_forces_refetch() {
    let discovery = stub_discovery_cache();
    let issuer = "https://idp.example.com";

    // Populate cache
    let _ = discovery.get(issuer).unwrap();
    assert_eq!(discovery.fetch_count(), 1);

    // Clear and re-get
    discovery.clear(issuer);
    let _ = discovery.get(issuer).unwrap();
    assert_eq!(discovery.fetch_count(), 2);
}

// ---------------------------------------------------------------------------
// TokenSet Display contract
// ---------------------------------------------------------------------------

/// Contract: hpc-auth.md § TokenSet Display
/// Spec: Auth7 — refresh tokens never appear in Display output
/// If this test didn't exist: debug logging or error messages could leak
/// refresh tokens, which have long lifetimes and high privilege.
#[test]
fn token_set_display_redacts_refresh_token() {
    let tokens = test_token_set();
    let display = format!("{}", tokens);

    assert!(!display.contains(tokens.refresh_token.as_deref().unwrap()));
    assert!(display.contains("REDACTED") || display.contains("[redacted]") || display.contains("***"));
}

// ---------------------------------------------------------------------------
// Error type contracts
// ---------------------------------------------------------------------------

/// Contract: hpc-auth.md § AuthError
/// Spec: each error variant is distinct and matchable
/// If this test didn't exist: callers couldn't distinguish between
/// network failures, expired tokens, and corrupted caches, making
/// error handling and user-facing messages impossible.
#[test]
fn errors_are_distinct() {
    let idp_err = AuthError::IdpUnreachable("timeout".into());
    let no_flow_err = AuthError::NoSupportedFlow;
    let expired_err = AuthError::TokenExpired;
    let corrupt_err = AuthError::CacheCorrupted("bad json".into());
    let perm_err = AuthError::CachePermissionDenied("/home/user/.cache/pact/tokens".into());
    let oauth_err = AuthError::OAuthFailed("invalid_grant".into());
    let timeout_err = AuthError::Timeout;
    let stale_err = AuthError::StaleDiscovery;

    // Each variant is distinguishable via pattern matching
    assert_matches!(idp_err, AuthError::IdpUnreachable(_));
    assert_matches!(no_flow_err, AuthError::NoSupportedFlow);
    assert_matches!(expired_err, AuthError::TokenExpired);
    assert_matches!(corrupt_err, AuthError::CacheCorrupted(_));
    assert_matches!(perm_err, AuthError::CachePermissionDenied(_));
    assert_matches!(oauth_err, AuthError::OAuthFailed(_));
    assert_matches!(timeout_err, AuthError::Timeout);
    assert_matches!(stale_err, AuthError::StaleDiscovery);
}
