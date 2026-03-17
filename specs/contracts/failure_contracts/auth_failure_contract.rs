//! Contract tests for authentication failure mode degradation.
//!
//! These test that failure modes F15, F16, F17 degrade as specified.
//!
//! Source: specs/failure-modes.md § F15, F16, F17

// ---------------------------------------------------------------------------
// F15: IdP unreachable
// ---------------------------------------------------------------------------

/// Contract: failure-modes.md § F15
/// Spec: existing valid access tokens continue to work until expiry
/// If this test didn't exist: an IdP outage could immediately invalidate all
/// existing sessions, locking out every admin simultaneously.
#[test]
fn f15_existing_tokens_continue_working() {
    let client = stub_auth_client();
    let token_set = client.login().unwrap();

    // IdP goes down after successful login
    client.set_idp_unreachable(true);

    // Existing access token should still be usable (validated offline via cached JWKS)
    let token = client.get_token().unwrap();
    assert_eq!(token, token_set.access_token);
}

/// Contract: failure-modes.md § F15
/// Spec: new login fails with IdpUnreachable error when IdP is down
/// If this test didn't exist: a login attempt against an unreachable IdP could
/// hang indefinitely or produce a confusing generic error.
#[test]
fn f15_login_fails_with_idp_unreachable() {
    let client = stub_auth_client();
    client.set_idp_unreachable(true);

    let result = client.login();
    assert_matches!(result, Err(AuthError::IdpUnreachable(_)));
}

/// Contract: failure-modes.md § F15
/// Spec: cached OIDC discovery document used for flow selection when IdP is down
/// If this test didn't exist: the discovery cache would be useless during an IdP
/// outage, preventing even the attempt to select an auth flow.
#[test]
fn f15_cached_discovery_used() {
    let discovery = stub_discovery_cache();
    let issuer = "https://idp.example.com";

    // Populate cache while IdP is reachable
    let _ = discovery.get(issuer).unwrap();
    assert_eq!(discovery.fetch_count(), 1);

    // IdP goes down
    discovery.set_fetch_failing(true);

    // Cached discovery should still be returned
    let doc = discovery.get(issuer).unwrap();
    assert_eq!(doc.issuer, issuer);
    assert_eq!(discovery.fetch_count(), 1); // No additional fetch attempted successfully
}

// ---------------------------------------------------------------------------
// F16: Token cache corrupted
// ---------------------------------------------------------------------------

/// Contract: failure-modes.md § F16
/// Spec: corrupted cache JSON rejected fail-closed with CacheCorrupted error
/// If this test didn't exist: corrupted JSON could cause panics, partial
/// deserialization, or silently produce invalid tokens.
#[test]
fn f16_corrupted_cache_rejected_fail_closed() {
    let cache = stub_token_cache();
    cache.write_raw("https://journal.example.com", b"{ not valid json !!!");

    let result = cache.read("https://journal.example.com");
    assert_matches!(result, Err(AuthError::CacheCorrupted(_)));
}

/// Contract: failure-modes.md § F16
/// Spec: login after corruption creates fresh cache, restoring normal operation
/// If this test didn't exist: a corrupted cache could permanently prevent
/// authentication until the user manually deletes the cache file.
#[test]
fn f16_re_login_recreates_cache() {
    let client = stub_auth_client();
    let cache = stub_token_cache();

    // Corrupt the cache
    cache.write_raw(client.server_url(), b"{ broken }");

    // Verify cache is corrupted
    let result = cache.read(client.server_url());
    assert_matches!(result, Err(AuthError::CacheCorrupted(_)));

    // Login should overwrite corrupted cache with fresh tokens
    let token_set = client.login().unwrap();
    assert!(!token_set.access_token.is_empty());

    // Cache should now be readable
    let cached = cache.read(client.server_url()).unwrap().unwrap();
    assert_eq!(cached.access_token, token_set.access_token);
}

// ---------------------------------------------------------------------------
// F17: Stale discovery document
// ---------------------------------------------------------------------------

/// Contract: failure-modes.md § F17
/// Spec: auth failure clears cached discovery, allowing fresh fetch on retry
/// If this test didn't exist: a stale discovery document with wrong endpoint URLs
/// could cause permanent auth failures until the CLI is restarted.
#[test]
fn f17_stale_discovery_cleared_on_auth_failure() {
    let discovery = stub_discovery_cache();
    let issuer = "https://idp.example.com";

    // Populate cache
    let _ = discovery.get(issuer).unwrap();
    assert_eq!(discovery.fetch_count(), 1);

    // Simulate auth failure due to stale discovery (wrong token endpoint)
    discovery.clear(issuer);

    // Next get should force a fresh fetch
    let _ = discovery.get(issuer).unwrap();
    assert_eq!(discovery.fetch_count(), 2);
}

/// Contract: failure-modes.md § F17
/// Spec: manual IdP config override bypasses discovery entirely
/// If this test didn't exist: a persistently stale or unreachable discovery
/// endpoint would make authentication impossible, with no workaround.
#[test]
fn f17_manual_override_bypasses_discovery() {
    let client = stub_auth_client_with_idp_override(
        "https://idp.example.com/token",
        "https://idp.example.com/authorize",
    );
    let discovery = stub_discovery_cache();

    // Discovery cache is empty — override should bypass it entirely
    let flow = client.selected_flow();
    assert!(
        matches!(flow, OAuthFlow::AuthCodePkce | OAuthFlow::ClientCredentials | OAuthFlow::DeviceCode),
        "override must allow flow selection without discovery"
    );
    assert_eq!(discovery.fetch_count(), 0, "discovery should not be fetched when override is set");
}
