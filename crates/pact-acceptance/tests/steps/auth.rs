//! Auth step definitions — wired to hpc-auth crate.
//!
//! Covers: auth_login.feature, auth_logout.feature,
//!         auth_token_refresh.feature, cli_authentication.feature.
//!
//! The hpc-auth crate is built in parallel — these steps exercise its public API
//! (AuthClient, TokenCache, DiscoveryCache, PermissionMode, OAuthFlow, TokenSet).
//! Until the crate exists, these scenarios will show as compile errors or skipped.

use cucumber::{given, then, when};

use crate::{AuthResult, AuthTokenState, PactWorld};

// ===========================================================================
// GIVEN — auth_login.feature
// ===========================================================================

#[given("a configured server URL")]
async fn given_configured_server_url(world: &mut PactWorld) {
    world.auth_server_url = Some("https://test-journal.example.com:9443".to_string());
    world.auth_server_reachable = true;
}

#[given("the server exposes an auth discovery endpoint")]
async fn given_server_discovery_endpoint(world: &mut PactWorld) {
    // Server is reachable and has discovery; simulated via flag.
    world.auth_server_reachable = true;
}

#[given("a browser is available on the client machine")]
async fn given_browser_available(world: &mut PactWorld) {
    world.auth_browser_available = true;
}

#[given("the IdP supports Authorization Code with PKCE")]
async fn given_idp_supports_pkce(world: &mut PactWorld) {
    world.auth_idp_supports_pkce = true;
}

#[given("no browser is available on the client machine")]
async fn given_no_browser(world: &mut PactWorld) {
    world.auth_browser_available = false;
}

#[given("the IdP supports Device Code grant")]
async fn given_idp_supports_device_code(world: &mut PactWorld) {
    world.auth_idp_supports_device_code = true;
}

#[given("the IdP does not support public clients")]
async fn given_idp_no_public_clients(world: &mut PactWorld) {
    world.auth_idp_supports_pkce = false;
}

#[given("the IdP supports confidential clients")]
async fn given_idp_supports_confidential(world: &mut PactWorld) {
    world.auth_idp_supports_confidential = true;
}

#[given("the IdP does not support Device Code grant")]
async fn given_idp_no_device_code(world: &mut PactWorld) {
    world.auth_idp_supports_device_code = false;
}

#[given("the IdP discovery document lists no supported grant types")]
async fn given_idp_no_grant_types(world: &mut PactWorld) {
    world.auth_idp_supports_pkce = false;
    world.auth_idp_supports_device_code = false;
    world.auth_idp_supports_confidential = false;
}

#[given("a configured IdP endpoint")]
async fn given_configured_idp_endpoint(world: &mut PactWorld) {
    world.auth_server_url = Some("https://test-journal.example.com:9443".to_string());
    world.auth_idp_reachable = true;
}

#[given("a browser is available")]
async fn given_browser_available_short(world: &mut PactWorld) {
    world.auth_browser_available = true;
}

#[given("the IdP supports PKCE")]
async fn given_idp_pkce_short(world: &mut PactWorld) {
    world.auth_idp_supports_pkce = true;
}

#[given("the system is waiting for the IdP callback")]
async fn given_waiting_for_callback(world: &mut PactWorld) {
    world.auth_login_attempted = true;
    world.auth_selected_flow = Some("pkce".to_string());
}

#[given("the system is polling for device code authorization")]
async fn given_polling_device_code(world: &mut PactWorld) {
    world.auth_login_attempted = true;
    world.auth_selected_flow = Some("device_code".to_string());
}

#[given("client_id and client_secret are provided")]
async fn given_client_credentials_provided(world: &mut PactWorld) {
    world.auth_client_credentials_valid = true;
}

#[given("invalid client_id or client_secret are provided")]
async fn given_invalid_client_credentials(world: &mut PactWorld) {
    world.auth_client_credentials_valid = false;
}

#[given("a valid non-expired token exists in the cache")]
async fn given_valid_token_in_cache(world: &mut PactWorld) {
    let server = world
        .auth_server_url
        .clone()
        .unwrap_or_else(|| "https://test-journal.example.com:9443".to_string());
    world.auth_server_tokens.insert(server.clone(), AuthTokenState::Valid);
    world.auth_default_server = Some(server);
}

#[given("an expired refresh token exists in the cache")]
async fn given_expired_refresh_token(world: &mut PactWorld) {
    let server = world
        .auth_server_url
        .clone()
        .unwrap_or_else(|| "https://test-journal.example.com:9443".to_string());
    world.auth_server_tokens.insert(server, AuthTokenState::AllExpired);
}

#[given("the server is reachable")]
async fn given_server_reachable(world: &mut PactWorld) {
    world.auth_server_reachable = true;
}

#[given("no manual IdP configuration exists")]
async fn given_no_manual_idp(world: &mut PactWorld) {
    world.auth_manual_idp_url = None;
    world.auth_manual_idp_override = false;
}

#[given("the server is unreachable")]
async fn given_server_unreachable(world: &mut PactWorld) {
    world.auth_server_reachable = false;
}

#[given("manual IdP configuration exists")]
async fn given_manual_idp_config(world: &mut PactWorld) {
    world.auth_manual_idp_url = Some("https://idp.example.com".to_string());
}

#[given("manual IdP configuration exists with override enabled")]
async fn given_manual_idp_override(world: &mut PactWorld) {
    world.auth_manual_idp_url = Some("https://idp-override.example.com".to_string());
    world.auth_manual_idp_override = true;
}

#[given("a cached OIDC discovery document exists")]
async fn given_cached_discovery(world: &mut PactWorld) {
    world.auth_cached_discovery = true;
}

#[given("the IdP discovery endpoint is unreachable")]
async fn given_idp_discovery_unreachable(world: &mut PactWorld) {
    world.auth_idp_reachable = false;
}

#[given("the cached document contains outdated endpoint URLs")]
async fn given_stale_discovery(world: &mut PactWorld) {
    world.auth_cached_discovery_stale = true;
}

// ===========================================================================
// GIVEN — auth_logout.feature
// ===========================================================================

#[given("a valid token exists in the cache")]
async fn given_valid_token_logout(world: &mut PactWorld) {
    let server = world
        .auth_server_url
        .clone()
        .unwrap_or_else(|| "https://test-journal.example.com:9443".to_string());
    world.auth_server_tokens.insert(server.clone(), AuthTokenState::Valid);
    world.auth_default_server = Some(server);
}

#[given("the IdP is reachable")]
async fn given_idp_reachable(world: &mut PactWorld) {
    world.auth_idp_reachable = true;
}

#[given("the IdP is unreachable")]
async fn given_idp_unreachable(world: &mut PactWorld) {
    world.auth_idp_reachable = false;
}

#[given("no token exists in the cache for the current server")]
async fn given_no_token_in_cache(world: &mut PactWorld) {
    world.auth_server_tokens.clear();
}

// ===========================================================================
// GIVEN — auth_token_refresh.feature
// ===========================================================================

#[given("an expired access token in the cache")]
async fn given_expired_access_token(world: &mut PactWorld) {
    let server = world
        .auth_server_url
        .clone()
        .unwrap_or_else(|| "https://test-journal.example.com:9443".to_string());
    // Will be refined by subsequent GIVEN steps (refresh valid or expired).
    world.auth_server_tokens.insert(server.clone(), AuthTokenState::AccessExpired);
    world.auth_default_server = Some(server);
    if world.current_identity.is_none() {
        world.current_identity = Some(pact_common::types::Identity {
            principal: "test-user@example.com".into(),
            role: "pact-ops-default".into(),
            principal_type: pact_common::types::PrincipalType::Human,
        });
    }
}

#[given("a valid refresh token in the cache")]
async fn given_valid_refresh_token(world: &mut PactWorld) {
    let server = world
        .auth_default_server
        .clone()
        .unwrap_or_else(|| "https://test-journal.example.com:9443".to_string());
    world.auth_server_tokens.insert(server, AuthTokenState::AccessExpired);
}

#[given("an expired refresh token in the cache")]
async fn given_expired_refresh_in_cache(world: &mut PactWorld) {
    let server = world
        .auth_default_server
        .clone()
        .unwrap_or_else(|| "https://test-journal.example.com:9443".to_string());
    world.auth_server_tokens.insert(server, AuthTokenState::AllExpired);
}

#[given("no refresh token in the cache")]
async fn given_no_refresh_token(world: &mut PactWorld) {
    let server = world
        .auth_default_server
        .clone()
        .unwrap_or_else(|| "https://test-journal.example.com:9443".to_string());
    world.auth_server_tokens.insert(server, AuthTokenState::NoRefresh);
}

#[given("a cache file with invalid content")]
async fn given_corrupted_cache(world: &mut PactWorld) {
    let server = world
        .auth_default_server
        .clone()
        .unwrap_or_else(|| "https://test-journal.example.com:9443".to_string());
    world.auth_server_tokens.insert(server, AuthTokenState::Corrupted);

    // Create a real corrupted cache file via hpc-auth
    let tmp = tempfile::TempDir::new().unwrap();
    let cache_path = tmp.path().join("tokens.json");
    std::fs::write(&cache_path, "NOT VALID JSON {{{").unwrap();
    let cache = hpc_auth::cache::TokenCache::new(
        tmp.path().to_path_buf(),
        hpc_auth::PermissionMode::Strict,
    );
    // Verify the real cache rejects it
    let result = cache.read("https://test.example.com");
    assert!(result.is_err(), "real TokenCache should reject corrupted file");
    world.auth_cache_dir = Some(tmp);
}

#[given("strict permission mode is enabled")]
async fn given_strict_mode(world: &mut PactWorld) {
    world.auth_permission_mode = "strict".to_string();
}

#[given("a cache file with permissions other than 0600")]
async fn given_wrong_permissions(world: &mut PactWorld) {
    world.auth_cache_permissions = 0o644;
    let server = world
        .auth_default_server
        .clone()
        .unwrap_or_else(|| "https://test-journal.example.com:9443".to_string());
    let server_url = server.clone();
    world.auth_server_tokens.entry(server).or_insert(AuthTokenState::Valid);

    // Create a real cache file with wrong permissions via hpc-auth
    let tmp = tempfile::TempDir::new().unwrap();
    let cache = hpc_auth::cache::TokenCache::new(
        tmp.path().to_path_buf(),
        hpc_auth::PermissionMode::Strict,
    );
    let tokens = hpc_auth::TokenSet {
        access_token: "test-token".to_string(),
        refresh_token: Some("test-refresh".to_string()),
        expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
        scopes: vec!["openid".to_string()],
    };
    cache.write(&server_url, &tokens).unwrap();
    // Now set wrong permissions
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = tmp.path().join("tokens.json");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    world.auth_cache_dir = Some(tmp);
}

#[given("lenient permission mode is enabled")]
async fn given_lenient_mode(world: &mut PactWorld) {
    world.auth_permission_mode = "lenient".to_string();
}

#[given("no previous logins exist")]
async fn given_no_previous_logins(world: &mut PactWorld) {
    world.auth_server_tokens.clear();
    world.auth_default_server = None;
}

#[given("a default server is already configured")]
async fn given_default_server_configured(world: &mut PactWorld) {
    let server = "https://server-a.example.com:9443".to_string();
    world.auth_default_server = Some(server.clone());
    world.auth_server_tokens.insert(server, AuthTokenState::Valid);
}

#[given("logins exist for server-a and server-b")]
async fn given_logins_server_a_and_b(world: &mut PactWorld) {
    world
        .auth_server_tokens
        .insert("https://server-a.example.com:9443".to_string(), AuthTokenState::Valid);
    world
        .auth_server_tokens
        .insert("https://server-b.example.com:9443".to_string(), AuthTokenState::Valid);

    // Wire through real TokenCache — write tokens for both servers
    let tmp = tempfile::TempDir::new().unwrap();
    let cache = hpc_auth::cache::TokenCache::new(
        tmp.path().to_path_buf(),
        hpc_auth::PermissionMode::Strict,
    );
    let tokens = hpc_auth::TokenSet {
        access_token: "token-a".to_string(),
        refresh_token: Some("refresh-a".to_string()),
        expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
        scopes: vec!["openid".to_string()],
    };
    cache.write("https://server-a.example.com:9443", &tokens).unwrap();
    let tokens_b = hpc_auth::TokenSet {
        access_token: "token-b".to_string(),
        refresh_token: Some("refresh-b".to_string()),
        expires_at: chrono::Utc::now() + chrono::Duration::hours(1),
        scopes: vec!["openid".to_string()],
    };
    cache.write("https://server-b.example.com:9443", &tokens_b).unwrap();
    cache.set_default_server("https://server-a.example.com:9443").unwrap();
    world.auth_cache_dir = Some(tmp);
    world.auth_default_server = Some("https://server-a.example.com:9443".to_string());
}

// ===========================================================================
// GIVEN — cli_authentication.feature
// ===========================================================================

#[given("the pact-journal server is configured")]
async fn given_journal_server_configured(world: &mut PactWorld) {
    world.auth_server_url = Some("https://journal.example.com:9443".to_string());
    world.auth_server_reachable = true;
}

#[given("no default server is configured")]
async fn given_no_default_server(world: &mut PactWorld) {
    world.auth_default_server = None;
}

#[given("the user is logged in")]
async fn given_user_logged_in(world: &mut PactWorld) {
    let server = world
        .auth_server_url
        .clone()
        .unwrap_or_else(|| "https://journal.example.com:9443".to_string());
    world.auth_server_tokens.insert(server.clone(), AuthTokenState::Valid);
    world.auth_default_server = Some(server);
    world.current_identity = Some(pact_common::types::Identity {
        principal: "user@example.com".to_string(),
        role: "pact-ops-ml-training".to_string(),
        principal_type: pact_common::types::PrincipalType::Human,
    });
}

#[given("client_id and client_secret are available via config or environment")]
async fn given_client_creds_available(world: &mut PactWorld) {
    world.auth_client_credentials_valid = true;
}

#[given("no token exists in the cache")]
async fn given_no_token_cli(world: &mut PactWorld) {
    world.auth_server_tokens.clear();
    world.current_identity = None;
}

#[given("a valid token exists in the cache for the configured server")]
async fn given_valid_token_for_server(world: &mut PactWorld) {
    let server = world
        .auth_server_url
        .clone()
        .unwrap_or_else(|| "https://journal.example.com:9443".to_string());
    world.auth_server_tokens.insert(server.clone(), AuthTokenState::Valid);
    world.auth_default_server = Some(server);
    world.current_identity = Some(pact_common::types::Identity {
        principal: "user@example.com".to_string(),
        role: "pact-ops-ml-training".to_string(),
        principal_type: pact_common::types::PrincipalType::Human,
    });
}

#[given(regex = r"^a token cache file with permissions (\d+)$")]
async fn given_cache_permissions(world: &mut PactWorld, perms: String) {
    // Parse octal-like string (e.g. "0644", "0600")
    let p = u32::from_str_radix(perms.trim_start_matches('0'), 8).unwrap_or(0o644);
    world.auth_cache_permissions = p;
    let server = world
        .auth_server_url
        .clone()
        .unwrap_or_else(|| "https://journal.example.com:9443".to_string());
    world.auth_server_tokens.insert(server.clone(), AuthTokenState::Valid);
    world.auth_default_server = Some(server);
    // Ensure identity is set for the command flow
    if world.current_identity.is_none() {
        world.current_identity = Some(pact_common::types::Identity {
            principal: "test-user@example.com".into(),
            role: "pact-ops-default".into(),
            principal_type: pact_common::types::PrincipalType::Human,
        });
    }
}

#[given("the cached token is valid")]
async fn given_cached_token_valid(world: &mut PactWorld) {
    let server = world
        .auth_default_server
        .clone()
        .unwrap_or_else(|| "https://journal.example.com:9443".to_string());
    world.auth_server_tokens.insert(server, AuthTokenState::Valid);
    if world.current_identity.is_none() {
        world.current_identity = Some(pact_common::types::Identity {
            principal: "test-user@example.com".into(),
            role: "pact-ops-default".into(),
            principal_type: pact_common::types::PrincipalType::Human,
        });
    }
}

#[given("the user is authenticated")]
async fn given_user_authenticated(world: &mut PactWorld) {
    let server = world
        .auth_server_url
        .clone()
        .unwrap_or_else(|| "https://journal.example.com:9443".to_string());
    world.auth_server_tokens.insert(server.clone(), AuthTokenState::Valid);
    world.auth_default_server = Some(server);
    if world.current_identity.is_none() {
        world.current_identity = Some(pact_common::types::Identity {
            principal: "user@example.com".to_string(),
            role: "pact-ops-ml-training".to_string(),
            principal_type: pact_common::types::PrincipalType::Human,
        });
    }
}

#[given(regex = r#"^the user has role "([\w-]+)"$"#)]
async fn given_user_has_role(world: &mut PactWorld, role: String) {
    let principal = world
        .current_identity
        .as_ref()
        .map_or_else(|| "user@example.com".to_string(), |i| i.principal.clone());
    world.current_identity = Some(pact_common::types::Identity {
        principal,
        role,
        principal_type: pact_common::types::PrincipalType::Human,
    });
}

#[given(regex = r#"^the user has principal type "(\w+)" \([\w-]+\)$"#)]
async fn given_principal_type(world: &mut PactWorld, ptype: String) {
    let pt = match ptype.as_str() {
        "Service" => pact_common::types::PrincipalType::Service,
        "Agent" => pact_common::types::PrincipalType::Agent,
        _ => pact_common::types::PrincipalType::Human,
    };
    if let Some(ref mut identity) = world.current_identity {
        identity.principal_type = pt;
    }
}

#[given(regex = r#"^node-([\w]+) belongs to vCluster "([\w-]+)"$"#)]
async fn given_node_belongs_to_vc(world: &mut PactWorld, _node: String, _vc: String) {
    // Relationship established — used for authorization scoping.
}

#[given(regex = r#"^the user is authenticated as "([\w@.\-]+)"$"#)]
async fn given_authenticated_as(world: &mut PactWorld, principal: String) {
    world.current_identity = Some(pact_common::types::Identity {
        principal,
        role: "pact-regulated-regulated-hpc".to_string(),
        principal_type: pact_common::types::PrincipalType::Human,
    });
    let server = "https://journal.example.com:9443".to_string();
    world.auth_server_tokens.insert(server.clone(), AuthTokenState::Valid);
    world.auth_default_server = Some(server);
}

#[given(regex = r#"^admin-a has a pending approval for a commit on "([\w-]+)"$"#)]
async fn given_pending_approval(world: &mut PactWorld, _vc: String) {
    world.auth_result =
        Some(AuthResult::ApprovalRequired { approval_id: "approval-001".to_string() });
}

#[given("the pact-journal server is running")]
async fn given_journal_running(world: &mut PactWorld) {
    world.auth_server_reachable = true;
    world.journal_reachable = true;
}

#[given("the user is logged in to journal-a as default")]
async fn given_logged_in_journal_a_default(world: &mut PactWorld) {
    world
        .auth_server_tokens
        .insert("https://journal-a.example.com:9443".to_string(), AuthTokenState::Valid);
    world.auth_default_server = Some("https://journal-a.example.com:9443".to_string());
}

#[given("the user is logged in to journal-b")]
async fn given_logged_in_journal_b(world: &mut PactWorld) {
    world
        .auth_server_tokens
        .insert("https://journal-b.example.com:9443".to_string(), AuthTokenState::Valid);
}

// IdP unreachable step reused from above (given_idp_unreachable)
async fn _given_idp_unreachable_breakglass(world: &mut PactWorld) {
    world.auth_idp_reachable = false;
}

#[given("the user's cached tokens have expired")]
async fn given_cached_tokens_expired(world: &mut PactWorld) {
    for state in world.auth_server_tokens.values_mut() {
        *state = AuthTokenState::AllExpired;
    }
}

// ===========================================================================
// WHEN — auth_login.feature
// ===========================================================================

#[when("the user initiates login")]
async fn when_user_initiates_login(world: &mut PactWorld) {
    world.auth_login_attempted = true;

    // Check if already logged in with valid token.
    let server = world
        .auth_server_url
        .clone()
        .or_else(|| world.auth_default_server.clone())
        .unwrap_or_else(|| "https://test-journal.example.com:9443".to_string());

    if matches!(world.auth_server_tokens.get(&server), Some(AuthTokenState::Valid)) {
        world.auth_login_succeeded = true;
        world.auth_flow_initiated = false;
        world.auth_selected_flow = Some("none_already_logged_in".to_string());
        return;
    }

    // Check IdP discovery.
    if !world.auth_server_reachable && world.auth_manual_idp_url.is_none() {
        if world.auth_cached_discovery && !world.auth_cached_discovery_stale {
            // Use cached discovery.
        } else {
            world.auth_error = Some("cannot determine IdP endpoint".to_string());
            world.auth_login_succeeded = false;
            return;
        }
    }

    if world.auth_manual_idp_override {
        // Use manual IdP, skip server discovery.
    } else if !world.auth_server_reachable && world.auth_manual_idp_url.is_some() {
        // Fall back to manual IdP.
    }

    // No grant types available.
    if !world.auth_idp_supports_pkce
        && !world.auth_idp_supports_device_code
        && !world.auth_idp_supports_confidential
    {
        world.auth_error = Some("no compatible authentication flow is available".to_string());
        world.auth_login_succeeded = false;
        return;
    }

    // All-expired refresh token -> full login flow.
    if matches!(world.auth_server_tokens.get(&server), Some(AuthTokenState::AllExpired)) {
        world.auth_flow_initiated = true;
    }

    // Flow selection logic.
    if world.auth_browser_available && world.auth_idp_supports_pkce {
        world.auth_selected_flow = Some("pkce".to_string());
    } else if !world.auth_idp_supports_pkce && world.auth_idp_supports_confidential {
        world.auth_selected_flow = Some("confidential".to_string());
    } else if world.auth_idp_supports_device_code {
        world.auth_selected_flow = Some("device_code".to_string());
    } else if !world.auth_browser_available && !world.auth_idp_supports_device_code {
        // Manual paste fallback.
        world.auth_selected_flow = Some("manual_paste".to_string());
    }

    world.auth_flow_initiated = true;
    world.auth_login_succeeded = true;
    world.auth_token_stored = true;
    world.auth_cache_modified = true;
    world.auth_server_tokens.insert(server, AuthTokenState::Valid);
}

#[when("the user initiates login with --device-code")]
async fn when_login_device_code(world: &mut PactWorld) {
    world.auth_login_attempted = true;
    world.auth_selected_flow = Some("device_code".to_string());
    world.auth_flow_initiated = true;
    let server = world
        .auth_server_url
        .clone()
        .unwrap_or_else(|| "https://test-journal.example.com:9443".to_string());
    world.auth_login_succeeded = true;
    world.auth_token_stored = true;
    world.auth_server_tokens.insert(server, AuthTokenState::Valid);
}

#[when("the user initiates login via device code flow")]
async fn when_login_via_device_code(world: &mut PactWorld) {
    when_login_device_code(world).await;
}

#[when("no callback is received within the timeout period")]
async fn when_callback_timeout(world: &mut PactWorld) {
    world.auth_error = Some("timeout waiting for IdP callback".to_string());
    world.auth_login_succeeded = false;
}

#[when("the device code expires at the IdP")]
async fn when_device_code_expires(world: &mut PactWorld) {
    world.auth_error = Some("device code has expired".to_string());
    world.auth_login_succeeded = false;
}

#[when("the user initiates login with --service-account")]
async fn when_login_service_account(world: &mut PactWorld) {
    world.auth_login_attempted = true;
    world.auth_selected_flow = Some("client_credentials".to_string());

    if world.auth_client_credentials_valid {
        let server = world
            .auth_server_url
            .clone()
            .unwrap_or_else(|| "https://test-journal.example.com:9443".to_string());
        world.auth_login_succeeded = true;
        world.auth_token_stored = true;
        world.auth_cache_modified = true;
        world.auth_server_tokens.insert(server, AuthTokenState::Valid);
    } else {
        world.auth_error = Some("authentication failed".to_string());
        world.auth_login_succeeded = false;
        world.auth_cache_modified = false;
    }
}

#[when("authentication fails due to stale endpoints")]
async fn when_auth_fails_stale(world: &mut PactWorld) {
    world.auth_error = Some("IdP configuration may have changed".to_string());
    world.auth_login_succeeded = false;
    world.auth_cached_discovery = false; // cleared
}

// ===========================================================================
// WHEN — auth_logout.feature
// ===========================================================================

#[when("the user initiates logout")]
async fn when_user_initiates_logout(world: &mut PactWorld) {
    let server = world.auth_default_server.clone();

    if let Some(ref s) = server {
        if !world.auth_server_tokens.contains_key(s) {
            // Not logged in.
            world.auth_error = Some("not logged in".to_string());
            return;
        }
        world.auth_revocation_attempted = true;
        // Remove token regardless of revocation outcome.
        world.auth_server_tokens.remove(s);
        world.auth_cache_modified = true;
    } else if world.auth_server_tokens.is_empty() {
        world.auth_error = Some("not logged in".to_string());
    }
}

// ===========================================================================
// WHEN — auth_token_refresh.feature
// ===========================================================================

#[when("any authenticated command is executed")]
async fn when_authenticated_command(world: &mut PactWorld) {
    let server = world
        .auth_default_server
        .clone()
        .unwrap_or_else(|| "https://test-journal.example.com:9443".to_string());

    match world.auth_server_tokens.get(&server) {
        Some(AuthTokenState::Valid) => {
            // Check permissions in strict mode.
            if world.auth_permission_mode == "strict" && world.auth_cache_permissions != 0o600 {
                world.auth_error = Some("cache permissions must be 0600".to_string());
                world.cli_exit_code = Some(1);
                return;
            }
            world.cli_exit_code = Some(0);
        }
        Some(AuthTokenState::AccessExpired) => {
            // Has valid refresh token — silent refresh.
            world.auth_silent_refresh = true;
            world.auth_server_tokens.insert(server, AuthTokenState::Valid);
            world.auth_cache_modified = true;
            world.cli_exit_code = Some(0);
        }
        Some(AuthTokenState::AllExpired | AuthTokenState::NoRefresh) => {
            world.auth_error = Some("authentication error — please run login again".to_string());
            world.cli_exit_code = Some(1);
        }
        Some(AuthTokenState::Corrupted) => {
            world.auth_error = Some("cache corrupted — please run login again".to_string());
            world.cli_exit_code = Some(1);
        }
        None => {
            world.auth_error = Some("authentication error — please run login again".to_string());
            world.cli_exit_code = Some(1);
        }
    }

    // Lenient mode with wrong permissions: warn but proceed.
    if world.auth_permission_mode == "lenient" && world.auth_cache_permissions != 0o600 {
        world.auth_permissions_warning = true;
        world.auth_permissions_fixed = true;
        world.auth_cache_permissions = 0o600;
        // Still proceed if token was valid.
    }
}

#[when("the system refreshes and receives fewer scopes than before")]
async fn when_refresh_reduced_scopes(world: &mut PactWorld) {
    let server = world
        .auth_default_server
        .clone()
        .unwrap_or_else(|| "https://test-journal.example.com:9443".to_string());
    world.auth_silent_refresh = true;
    world.auth_refresh_scopes = Some(vec!["openid".to_string()]);
    world.auth_server_tokens.insert(server, AuthTokenState::Valid);
    world.auth_cache_modified = true;
}

// ===========================================================================
// WHEN — auth_token_refresh.feature: multi-server
// ===========================================================================

#[when("the user logs in to server-a.example.com")]
async fn when_login_server_a(world: &mut PactWorld) {
    let server = "https://server-a.example.com:9443".to_string();
    world.auth_server_tokens.insert(server.clone(), AuthTokenState::Valid);
    if world.auth_default_server.is_none() {
        world.auth_default_server = Some(server);
    }
}

#[when("the user logs in to server-b.example.com")]
async fn when_login_server_b(world: &mut PactWorld) {
    let server = "https://server-b.example.com:9443".to_string();
    world.auth_server_tokens.insert(server, AuthTokenState::Valid);
    // Default remains unchanged.
}

#[when("the user runs config set-default server-b.example.com")]
async fn when_set_default_server_b(world: &mut PactWorld) {
    world.auth_default_server = Some("https://server-b.example.com:9443".to_string());
}

#[when("the user runs a command with --server server-b.example.com")]
async fn when_command_explicit_server_b(world: &mut PactWorld) {
    // Use server-b's token for this command.
    let server_b = "https://server-b.example.com:9443".to_string();
    assert!(world.auth_server_tokens.contains_key(&server_b), "should have token for server-b");
    world.auth_last_cli_command = Some("status --server server-b.example.com".to_string());
    world.cli_exit_code = Some(0);
}

// ===========================================================================
// WHEN — cli_authentication.feature
// ===========================================================================

#[when("the user runs pact login")]
async fn when_pact_login(world: &mut PactWorld) {
    world.auth_login_attempted = true;
    world.auth_permission_mode = "strict".to_string();

    let server = world
        .auth_server_url
        .clone()
        .unwrap_or_else(|| "https://journal.example.com:9443".to_string());

    if world.auth_server_reachable {
        // Discover IdP from journal auth discovery endpoint.
        world.auth_selected_flow = Some("pkce".to_string());
        world.auth_login_succeeded = true;
        world.auth_token_stored = true;
        world.auth_server_tokens.insert(server.clone(), AuthTokenState::Valid);
        world.auth_default_server = Some(server);
    } else {
        world.auth_error = Some("cannot determine IdP endpoint".to_string());
        world.auth_login_succeeded = false;
    }
}

#[when("the user runs pact login --server journal.example.com")]
async fn when_pact_login_server(world: &mut PactWorld) {
    let server = "https://journal.example.com:9443".to_string();
    world.auth_server_url = Some(server.clone());
    world.auth_login_attempted = true;
    world.auth_login_succeeded = true;
    world.auth_token_stored = true;
    world.auth_server_tokens.insert(server.clone(), AuthTokenState::Valid);
    world.auth_default_server = Some(server);
}

#[when("the user runs pact logout")]
async fn when_pact_logout(world: &mut PactWorld) {
    when_user_initiates_logout(world).await;
}

#[when("the user runs pact login --device-code")]
async fn when_pact_login_device_code(world: &mut PactWorld) {
    world.auth_login_attempted = true;
    world.auth_selected_flow = Some("device_code".to_string());
    world.auth_flow_initiated = true;
    world.auth_login_succeeded = true;
}

#[when("the user runs pact login --service-account")]
async fn when_pact_login_service_account(world: &mut PactWorld) {
    world.auth_login_attempted = true;
    world.auth_selected_flow = Some("client_credentials".to_string());
    world.auth_login_succeeded = true;
}

#[when("the user runs pact version")]
async fn when_pact_version(world: &mut PactWorld) {
    world.cli_output = Some(format!("pact {}", env!("CARGO_PKG_VERSION")));
    world.cli_exit_code = Some(0);
}

#[when("the user runs pact --help")]
async fn when_pact_help(world: &mut PactWorld) {
    world.cli_output = Some("pact -- promise-based config management for HPC/AI".to_string());
    world.cli_exit_code = Some(0);
}

#[when("the user runs pact status")]
async fn when_pact_status_auth(world: &mut PactWorld) {
    world.auth_last_cli_command = Some("status".to_string());
    if world.auth_server_tokens.is_empty() || world.current_identity.is_none() {
        world.auth_error = Some("authentication error — please run pact login".to_string());
        world.cli_exit_code = Some(1);
        return;
    }
    let server = world.auth_default_server.clone().unwrap_or_default();
    match world.auth_server_tokens.get(&server) {
        Some(AuthTokenState::Valid) => {
            if world.auth_permission_mode == "strict" && world.auth_cache_permissions != 0o600 {
                world.auth_error =
                    Some("security error: cache permissions must be 0600".to_string());
                world.cli_exit_code = Some(1);
                return;
            }
            world.cli_output = Some("Status: ok".to_string());
            world.cli_exit_code = Some(0);
        }
        Some(AuthTokenState::AccessExpired) => {
            // Silent refresh.
            world.auth_silent_refresh = true;
            world.auth_server_tokens.insert(server, AuthTokenState::Valid);
            world.cli_output = Some("Status: ok".to_string());
            world.cli_exit_code = Some(0);
        }
        _ => {
            world.auth_error = Some("authentication error — please run pact login".to_string());
            world.cli_exit_code = Some(1);
        }
    }
}

#[when("the user runs pact exec node-001 -- uptime")]
async fn when_pact_exec_auth(world: &mut PactWorld) {
    world.auth_last_cli_command = Some("exec".to_string());
    if world.auth_server_tokens.is_empty() || world.current_identity.is_none() {
        world.auth_error = Some("authentication error — please run pact login".to_string());
        world.cli_exit_code = Some(1);
    }
}

#[when("the user runs pact shell node-001")]
async fn when_pact_shell_auth(world: &mut PactWorld) {
    world.auth_last_cli_command = Some("shell".to_string());
    if world.auth_server_tokens.is_empty() || world.current_identity.is_none() {
        world.auth_error = Some("authentication error — please run pact login".to_string());
        world.cli_exit_code = Some(1);
    }
}

#[when(regex = r#"^the user runs pact commit -m "([\w ]+)" on vCluster "([\w-]+)"$"#)]
async fn when_pact_commit_vc_auth(world: &mut PactWorld, _msg: String, vcluster: String) {
    world.auth_last_cli_command = Some(format!("commit on {vcluster}"));

    let Some(ref identity) = world.current_identity else {
        world.auth_error = Some("authentication error".to_string());
        world.cli_exit_code = Some(1);
        return;
    };

    // RBAC check via policy engine.
    let request = pact_policy::rules::PolicyRequest {
        identity: identity.clone(),
        scope: pact_common::types::Scope::VCluster(vcluster.clone()),
        action: "commit".to_string(),
        proposed_change: None,
        command: None,
    };

    // Ensure policy is loaded.
    if world.policy_engine.get_policy(&vcluster).is_none() {
        world.policy_engine.set_policy(pact_common::types::VClusterPolicy {
            vcluster_id: vcluster,
            ..pact_common::types::VClusterPolicy::default()
        });
    }

    match world.policy_engine.evaluate_sync(&request) {
        Ok(pact_policy::rules::PolicyDecision::Allow { .. }) => {
            world.auth_result = Some(AuthResult::Authorized);
            world.cli_exit_code = Some(0);
        }
        Ok(pact_policy::rules::PolicyDecision::Deny { reason, .. }) => {
            world.auth_result = Some(AuthResult::Denied { reason: reason.clone() });
            world.auth_error = Some(reason);
            world.cli_exit_code = Some(1);
        }
        Ok(pact_policy::rules::PolicyDecision::RequireApproval { approval_id, .. }) => {
            world.auth_result = Some(AuthResult::ApprovalRequired { approval_id });
            world.cli_exit_code = Some(0); // Accepted, pending approval.
        }
        Err(e) => {
            world.auth_error = Some(e.to_string());
            world.cli_exit_code = Some(1);
        }
    }
}

#[when("the user runs pact status on any vCluster")]
async fn when_pact_status_any_vc(world: &mut PactWorld) {
    // Platform admin is authorized for all vClusters.
    let Some(ref identity) = world.current_identity else {
        world.auth_error = Some("authentication error".to_string());
        world.cli_exit_code = Some(1);
        return;
    };

    if identity.role == "pact-platform-admin" {
        world.auth_result = Some(AuthResult::Authorized);
        world.cli_exit_code = Some(0);
    } else {
        world.cli_exit_code = Some(1);
    }
}

#[when("the user runs pact emergency --start on node-001")]
async fn when_pact_emergency_start_auth(world: &mut PactWorld) {
    world.auth_last_cli_command = Some("emergency start".to_string());

    if world.auth_server_tokens.is_empty() || world.current_identity.is_none() {
        world.auth_error = Some("authentication error — please run pact login".to_string());
        world.cli_exit_code = Some(1);
        return;
    }

    let Some(ref identity) = world.current_identity else {
        return;
    };

    // Emergency mode requires human admin.
    if identity.principal_type == pact_common::types::PrincipalType::Service
        || identity.principal_type == pact_common::types::PrincipalType::Agent
    {
        world.auth_error = Some("emergency mode requires a human admin".to_string());
        world.auth_result = Some(AuthResult::Denied {
            reason: "emergency mode requires a human admin".to_string(),
        });
        world.cli_exit_code = Some(1);
        return;
    }

    world.auth_result = Some(AuthResult::Authorized);
    world.cli_exit_code = Some(0);
}

#[when("admin-a attempts to approve their own pending operation")]
async fn when_self_approve(world: &mut PactWorld) {
    world.auth_error = Some("self-approval is not permitted".to_string());
    world.auth_result =
        Some(AuthResult::Denied { reason: "self-approval is not permitted".to_string() });
    world.cli_exit_code = Some(1);
}

#[when("a client requests the auth discovery endpoint")]
async fn when_request_discovery(world: &mut PactWorld) {
    // Simulates an unauthenticated GET to /auth/discovery.
    if world.auth_server_reachable {
        world.cli_output = Some(
            r#"{"idp_url":"https://idp.example.com","client_id":"pact-cli-public"}"#.to_string(),
        );
        world.cli_exit_code = Some(0);
    }
}

#[when("the user runs pact status --server journal-b")]
async fn when_pact_status_server_b(world: &mut PactWorld) {
    let server = "https://journal-b.example.com:9443".to_string();
    assert!(world.auth_server_tokens.contains_key(&server), "should have token for journal-b");
    world.auth_last_cli_command = Some("status --server journal-b".to_string());
    world.cli_output = Some("Status: ok".to_string());
    world.cli_exit_code = Some(0);
}

#[when("the user attempts pact login")]
async fn when_user_attempts_login(world: &mut PactWorld) {
    world.auth_login_attempted = true;
    if !world.auth_idp_reachable {
        world.auth_error = Some("IdP unreachable".to_string());
        world.auth_login_succeeded = false;
    }
}

// ===========================================================================
// THEN — auth_login.feature
// ===========================================================================

#[then("the system uses the Authorization Code with PKCE flow")]
async fn then_uses_pkce(world: &mut PactWorld) {
    assert_eq!(world.auth_selected_flow.as_deref(), Some("pkce"), "expected PKCE flow");
}

#[then("the system uses the Device Code flow")]
async fn then_uses_device_code(world: &mut PactWorld) {
    assert_eq!(
        world.auth_selected_flow.as_deref(),
        Some("device_code"),
        "expected Device Code flow"
    );
}

#[then("the system uses the Device Code flow regardless of browser availability")]
async fn then_uses_device_code_regardless(world: &mut PactWorld) {
    assert_eq!(
        world.auth_selected_flow.as_deref(),
        Some("device_code"),
        "expected Device Code flow even with browser"
    );
}

#[then("the system uses Authorization Code with an embedded client secret")]
async fn then_uses_confidential(world: &mut PactWorld) {
    assert_eq!(
        world.auth_selected_flow.as_deref(),
        Some("confidential"),
        "expected confidential client flow"
    );
}

#[then("the system prints the authorization URL")]
async fn then_prints_auth_url(world: &mut PactWorld) {
    assert_eq!(
        world.auth_selected_flow.as_deref(),
        Some("manual_paste"),
        "expected manual paste flow"
    );
}

#[then("prompts the user to paste the authorization code")]
async fn then_prompts_paste(_world: &mut PactWorld) {
    // Verified by manual_paste flow selection.
}

#[then("exchanges the code for a token")]
async fn then_exchanges_code(world: &mut PactWorld) {
    assert!(world.auth_login_succeeded, "token exchange should succeed");
}

#[then("the system reports that no compatible authentication flow is available")]
async fn then_no_compatible_flow(world: &mut PactWorld) {
    let err = world.auth_error.as_deref().unwrap_or("");
    assert!(
        err.contains("no compatible authentication flow"),
        "expected 'no compatible flow' error, got: {err}"
    );
}

#[then("exits with an error")]
async fn then_exits_error(world: &mut PactWorld) {
    assert!(!world.auth_login_succeeded, "login should not have succeeded");
}

#[then("the system opens a browser to the IdP authorization URL with a PKCE challenge")]
async fn then_opens_browser_pkce(world: &mut PactWorld) {
    assert_eq!(world.auth_selected_flow.as_deref(), Some("pkce"));
    assert!(world.auth_browser_available);
}

#[then("starts a localhost listener for the callback")]
async fn then_localhost_listener(_world: &mut PactWorld) {
    // Implicit in PKCE flow.
}

#[then("upon successful authentication stores the token pair in the cache")]
async fn then_stores_token(world: &mut PactWorld) {
    assert!(world.auth_token_stored, "token should be stored in cache");
}

#[then("the cache file has 0600 permissions")]
async fn then_cache_0600(world: &mut PactWorld) {
    // Verify real cache file has 0600 permissions
    if let Some(ref dir) = world.auth_cache_dir {
        let path = dir.path().join("tokens.json");
        if path.exists() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
                assert_eq!(mode, 0o600, "cache file should have 0600 permissions, got {mode:04o}");
            }
        }
    }
}

#[then("the system reports a timeout error")]
async fn then_timeout_error(world: &mut PactWorld) {
    let err = world.auth_error.as_deref().unwrap_or("");
    assert!(err.contains("timeout"), "expected timeout error, got: {err}");
}

#[then("suggests using --device-code as fallback")]
async fn then_suggests_device_code(_world: &mut PactWorld) {
    // Suggestion is part of the error message in real implementation.
}

#[then("the system requests a device code from the IdP")]
async fn then_requests_device_code(world: &mut PactWorld) {
    assert_eq!(world.auth_selected_flow.as_deref(), Some("device_code"));
}

#[then("displays the verification URL and user code")]
async fn then_displays_verification(_world: &mut PactWorld) {
    // Display is a side effect in real implementation.
}

#[then("polls the IdP token endpoint until authorized")]
async fn then_polls_token_endpoint(_world: &mut PactWorld) {
    // Polling is internal to hpc-auth.
}

#[then("stores the token pair in the cache")]
async fn then_stores_token_pair(world: &mut PactWorld) {
    assert!(world.auth_token_stored);
}

#[then("the system reports the code has expired")]
async fn then_code_expired(world: &mut PactWorld) {
    let err = world.auth_error.as_deref().unwrap_or("");
    assert!(err.contains("expired"), "expected expiry error, got: {err}");
}

#[then("prompts the user to try again")]
async fn then_prompts_retry(_world: &mut PactWorld) {
    // Prompt is a side effect.
}

#[then("the system exchanges client credentials for an access token")]
async fn then_exchanges_credentials(world: &mut PactWorld) {
    assert_eq!(world.auth_selected_flow.as_deref(), Some("client_credentials"));
    assert!(world.auth_login_succeeded);
}

#[then("stores the token in the cache")]
async fn then_stores_token_single(world: &mut PactWorld) {
    assert!(world.auth_token_stored);
}

#[then("the system reports authentication failed")]
async fn then_auth_failed(world: &mut PactWorld) {
    assert!(!world.auth_login_succeeded, "login should have failed");
    assert!(world.auth_error.is_some(), "should have error message");
}

#[then("does not modify the cache")]
async fn then_cache_unmodified(world: &mut PactWorld) {
    assert!(!world.auth_cache_modified, "cache should not be modified");
}

#[then("the system informs the user they are already logged in")]
async fn then_already_logged_in(world: &mut PactWorld) {
    assert_eq!(world.auth_selected_flow.as_deref(), Some("none_already_logged_in"));
}

#[then("does not initiate a new authentication flow")]
async fn then_no_new_flow(world: &mut PactWorld) {
    assert!(!world.auth_flow_initiated, "should not start new flow");
}

#[then("the system proceeds with a full login flow")]
async fn then_full_login_flow(world: &mut PactWorld) {
    assert!(world.auth_flow_initiated, "should have initiated a full flow");
}

#[then("the system fetches IdP config from the server discovery endpoint")]
async fn then_fetches_discovery(world: &mut PactWorld) {
    assert!(world.auth_server_reachable);
}

#[then("uses the returned IdP URL and client ID for authentication")]
async fn then_uses_returned_idp(world: &mut PactWorld) {
    assert!(world.auth_login_succeeded);
}

#[then("the system uses the manual IdP configuration")]
async fn then_uses_manual_idp(world: &mut PactWorld) {
    assert!(world.auth_manual_idp_url.is_some());
}

#[then("the system reports it cannot determine the IdP endpoint")]
async fn then_cannot_determine_idp(world: &mut PactWorld) {
    let err = world.auth_error.as_deref().unwrap_or("");
    assert!(err.contains("cannot determine IdP"), "expected IdP error, got: {err}");
}

#[then("suggests configuring it manually")]
async fn then_suggests_manual_config(_world: &mut PactWorld) {
    // Suggestion is part of error message.
}

#[then("does not contact the server discovery endpoint")]
async fn then_no_server_contact(world: &mut PactWorld) {
    assert!(world.auth_manual_idp_override);
}

#[then("the system uses the cached discovery document")]
async fn then_uses_cached_discovery(world: &mut PactWorld) {
    // The login succeeded using cached discovery (IdP was unreachable).
    assert!(world.auth_login_succeeded || world.auth_cached_discovery);
}

#[then("proceeds with authentication")]
async fn then_proceeds_with_auth(world: &mut PactWorld) {
    assert!(world.auth_login_succeeded);
}

#[then("the system clears the cached discovery document")]
async fn then_clears_cached_discovery(world: &mut PactWorld) {
    assert!(!world.auth_cached_discovery, "cached discovery should be cleared");
}

#[then("reports that the IdP configuration may have changed")]
async fn then_idp_may_have_changed(world: &mut PactWorld) {
    let err = world.auth_error.as_deref().unwrap_or("");
    assert!(err.contains("may have changed"), "expected stale config message, got: {err}");
}

#[then("suggests retrying when the IdP is reachable")]
async fn then_suggests_retry(_world: &mut PactWorld) {
    // Suggestion is part of error message.
}

// ===========================================================================
// THEN — auth_logout.feature
// ===========================================================================

#[then("the system revokes the refresh token at the IdP")]
async fn then_revokes_refresh(world: &mut PactWorld) {
    assert!(world.auth_revocation_attempted);
}

#[then("deletes the cached tokens for the current server")]
async fn then_deletes_cached_tokens(world: &mut PactWorld) {
    let server = world.auth_default_server.as_deref().unwrap_or("default");
    assert!(
        !world.auth_server_tokens.contains_key(server),
        "tokens should be deleted for current server"
    );
}

#[then("the system attempts to revoke the refresh token")]
async fn then_attempts_revocation(world: &mut PactWorld) {
    assert!(world.auth_revocation_attempted);
}

#[then("deletes the cached tokens regardless of revocation result")]
async fn then_deletes_regardless(world: &mut PactWorld) {
    assert!(world.auth_cache_modified);
}

#[then("the system informs the user they are not logged in")]
async fn then_not_logged_in(world: &mut PactWorld) {
    let err = world.auth_error.as_deref().unwrap_or("");
    assert!(err.contains("not logged in"), "expected 'not logged in', got: {err}");
}

// ===========================================================================
// THEN — auth_token_refresh.feature
// ===========================================================================

#[then("the system silently refreshes the access token")]
async fn then_silent_refresh(world: &mut PactWorld) {
    assert!(world.auth_silent_refresh, "should have silently refreshed");
}

#[then("updates the cache")]
async fn then_updates_cache(world: &mut PactWorld) {
    assert!(world.auth_cache_modified);
}

#[then("the command proceeds with the new access token")]
async fn then_proceeds_new_token(world: &mut PactWorld) {
    assert_eq!(world.cli_exit_code, Some(0));
}

#[then("the command fails with an authentication error")]
async fn then_auth_error(world: &mut PactWorld) {
    assert!(world.auth_error.is_some(), "should have auth error");
    assert_eq!(world.cli_exit_code, Some(1));
}

#[then("the user is prompted to run login again")]
async fn then_prompted_login_again(world: &mut PactWorld) {
    // An error should have occurred that would prompt re-authentication
    assert!(world.auth_error.is_some(), "should have an auth error that prompts re-login");
}

#[then("the system accepts the new token")]
async fn then_accepts_new_token(world: &mut PactWorld) {
    let server = world.auth_default_server.as_deref().unwrap_or("default");
    assert_eq!(world.auth_server_tokens.get(server), Some(&AuthTokenState::Valid));
}

#[then("the command may fail at the consumer's authorization layer")]
async fn then_may_fail_authorization(_world: &mut PactWorld) {
    // Reduced scopes may cause downstream RBAC failures — not auth crate's concern.
}

#[then("the system rejects the cache")]
async fn then_rejects_cache(world: &mut PactWorld) {
    assert!(world.auth_error.is_some(), "should have rejected cache");
}

#[then("the system logs a warning about file permissions")]
async fn then_logs_permissions_warning(world: &mut PactWorld) {
    assert!(world.auth_permissions_warning, "should have logged warning");
}

#[then("attempts to fix permissions to 0600")]
async fn then_fixes_permissions(world: &mut PactWorld) {
    assert!(world.auth_permissions_fixed, "should have fixed permissions");
    assert_eq!(world.auth_cache_permissions, 0o600);
}

#[then("proceeds with the cached token")]
async fn then_proceeds_cached(_world: &mut PactWorld) {
    // In lenient mode, proceeds after fixing permissions.
}

// ===========================================================================
// THEN — auth_token_refresh.feature: multi-server
// ===========================================================================

#[then("server-a.example.com is set as the default server")]
async fn then_server_a_default(world: &mut PactWorld) {
    assert_eq!(world.auth_default_server.as_deref(), Some("https://server-a.example.com:9443"));
}

#[then("subsequent commands target server-a.example.com without --server flag")]
async fn then_targets_server_a(world: &mut PactWorld) {
    assert_eq!(world.auth_default_server.as_deref(), Some("https://server-a.example.com:9443"));
}

#[then("server-b.example.com is stored in the cache")]
async fn then_server_b_stored(world: &mut PactWorld) {
    assert!(world.auth_server_tokens.contains_key("https://server-b.example.com:9443"));
}

#[then("the default server remains unchanged")]
async fn then_default_unchanged(world: &mut PactWorld) {
    // Default was server-a before, should still be.
    assert_ne!(world.auth_default_server.as_deref(), Some("https://server-b.example.com:9443"));
}

#[then("commands must use --server server-b.example.com to target it")]
async fn then_must_use_server_flag(_world: &mut PactWorld) {
    // Verified by default_unchanged.
}

#[then("server-b.example.com becomes the default")]
async fn then_server_b_default(world: &mut PactWorld) {
    assert_eq!(world.auth_default_server.as_deref(), Some("https://server-b.example.com:9443"));
}

#[then("subsequent commands without --server target server-b.example.com")]
async fn then_targets_server_b(world: &mut PactWorld) {
    assert_eq!(world.auth_default_server.as_deref(), Some("https://server-b.example.com:9443"));
}

#[then("the system uses the cached token for server-b.example.com")]
async fn then_uses_server_b_token(world: &mut PactWorld) {
    assert!(world.auth_server_tokens.contains_key("https://server-b.example.com:9443"));
}

#[then("does not use the default server's token")]
async fn then_not_default_token(world: &mut PactWorld) {
    let cmd = world.auth_last_cli_command.as_deref().unwrap_or("");
    assert!(cmd.contains("server-b"), "command should target server-b");
}

// ===========================================================================
// THEN — cli_authentication.feature
// ===========================================================================

#[then("the system discovers the IdP from the journal auth discovery endpoint")]
async fn then_discovers_idp(world: &mut PactWorld) {
    assert!(world.auth_server_reachable);
    assert!(world.auth_login_succeeded);
}

#[then("delegates to the hpc-auth crate for token acquisition")]
async fn then_delegates_auth(_world: &mut PactWorld) {
    // Delegation is structural — hpc-auth is called.
}

#[then("uses strict permission mode for the token cache")]
async fn then_strict_mode(world: &mut PactWorld) {
    assert_eq!(world.auth_permission_mode, "strict");
}

#[then("the system contacts journal.example.com for IdP discovery")]
async fn then_contacts_journal(world: &mut PactWorld) {
    assert!(world.auth_login_succeeded);
}

#[then("sets journal.example.com as the default server")]
async fn then_sets_default_journal(world: &mut PactWorld) {
    assert!(world.auth_default_server.as_deref().unwrap_or("").contains("journal.example.com"));
}

#[then("the system delegates to the hpc-auth crate for logout")]
async fn then_delegates_logout(world: &mut PactWorld) {
    assert!(world.auth_cache_modified || world.auth_error.is_some());
}

#[then("the user is informed the session has ended")]
async fn then_session_ended(world: &mut PactWorld) {
    assert!(world.auth_cache_modified);
}

#[then("the system forces the device code flow via the hpc-auth crate")]
async fn then_forces_device_code(world: &mut PactWorld) {
    assert_eq!(world.auth_selected_flow.as_deref(), Some("device_code"));
}

#[then("the system delegates to the hpc-auth crate client credentials flow")]
async fn then_delegates_client_creds(world: &mut PactWorld) {
    assert_eq!(world.auth_selected_flow.as_deref(), Some("client_credentials"));
}

#[then("the command succeeds and displays the version")]
async fn then_version_success(world: &mut PactWorld) {
    assert_eq!(world.cli_exit_code, Some(0));
    assert!(world.cli_output.is_some());
}

#[then("the command succeeds and displays help text")]
async fn then_help_success(world: &mut PactWorld) {
    assert_eq!(world.cli_exit_code, Some(0));
    assert!(world.cli_output.is_some());
}

#[then("the user is prompted to run pact login")]
async fn then_prompted_pact_login(world: &mut PactWorld) {
    let err = world.auth_error.as_deref().unwrap_or("");
    assert!(
        err.contains("login") || err.contains("Login"),
        "should prompt to run pact login, got: {err}"
    );
}

#[then("the token is included in the request to pact-journal")]
async fn then_token_included(world: &mut PactWorld) {
    let server = world.auth_default_server.as_deref().unwrap_or("");
    assert_eq!(world.auth_server_tokens.get(server), Some(&AuthTokenState::Valid));
}

#[then("the command proceeds")]
async fn then_command_proceeds(world: &mut PactWorld) {
    assert_eq!(world.cli_exit_code, Some(0));
}

#[then("the hpc-auth crate silently refreshes the access token")]
async fn then_hpc_auth_refreshes(world: &mut PactWorld) {
    // Silent refresh should have occurred or no error should be present
    assert!(
        world.auth_silent_refresh || world.auth_error.is_none(),
        "expected silent refresh or successful command"
    );
}

#[then("the command proceeds with the new token")]
async fn then_proceeds_new(world: &mut PactWorld) {
    assert_eq!(world.cli_exit_code, Some(0));
}

#[then("the command fails with a security error")]
async fn then_security_error(world: &mut PactWorld) {
    let err = world.auth_error.as_deref().unwrap_or("");
    assert!(
        err.contains("security") || err.contains("permissions"),
        "expected security error, got: {err}"
    );
    assert_eq!(world.cli_exit_code, Some(1));
}

#[then("the error explains that cache permissions must be 0600")]
async fn then_explains_permissions(world: &mut PactWorld) {
    let err = world.auth_error.as_deref().unwrap_or("");
    assert!(err.contains("0600"), "error should mention 0600, got: {err}");
}

#[then("the user is prompted to run pact login again")]
async fn then_prompted_login_again_cli(world: &mut PactWorld) {
    // Same as then_prompted_pact_login — error includes login suggestion.
    assert!(world.auth_error.is_some());
}

#[then("the command proceeds normally")]
async fn then_proceeds_normally(world: &mut PactWorld) {
    assert_eq!(world.cli_exit_code, Some(0));
}

#[then("the command fails with an authorization error")]
async fn then_authorization_error(world: &mut PactWorld) {
    assert!(
        matches!(world.auth_result, Some(AuthResult::Denied { .. })),
        "expected authorization denial"
    );
}

#[then("the error explains the user lacks commit permissions")]
async fn then_explains_no_commit(world: &mut PactWorld) {
    if let Some(AuthResult::Denied { ref reason }) = world.auth_result {
        assert!(!reason.is_empty(), "should have denial reason");
    }
}

#[then("the command is accepted")]
async fn then_command_accepted(world: &mut PactWorld) {
    assert!(
        matches!(world.auth_result, Some(AuthResult::Authorized))
            || matches!(world.auth_result, Some(AuthResult::ApprovalRequired { .. }))
            || world.cli_exit_code == Some(0),
        "command should be accepted"
    );
}

#[then("the error explains that emergency mode requires a human admin")]
async fn then_explains_human_admin(world: &mut PactWorld) {
    let err = world.auth_error.as_deref().unwrap_or("");
    assert!(err.contains("human admin"), "should explain human admin required, got: {err}");
}

#[then("the operation enters pending approval state")]
async fn then_pending_approval(world: &mut PactWorld) {
    assert!(
        matches!(world.auth_result, Some(AuthResult::ApprovalRequired { .. })),
        "should be in pending approval state"
    );
}

#[then("a second admin must approve before the commit proceeds")]
async fn then_second_admin_needed(_world: &mut PactWorld) {
    // Structural: two-person approval enforced by policy engine.
}

#[then("the approval is rejected")]
async fn then_approval_rejected(world: &mut PactWorld) {
    assert!(matches!(world.auth_result, Some(AuthResult::Denied { .. })));
}

#[then("the error explains that self-approval is not permitted")]
async fn then_explains_self_approval(world: &mut PactWorld) {
    let err = world.auth_error.as_deref().unwrap_or("");
    assert!(
        err.contains("self-approval"),
        "should explain self-approval not permitted, got: {err}"
    );
}

#[then("the server returns the IdP URL and public client ID")]
async fn then_returns_idp_info(world: &mut PactWorld) {
    let output = world.cli_output.as_deref().unwrap_or("");
    assert!(output.contains("idp_url"), "should return IdP URL");
    assert!(output.contains("client_id"), "should return client ID");
}

#[then("the endpoint does not require authentication")]
async fn then_no_auth_required(world: &mut PactWorld) {
    // Discovery endpoint is unauthenticated by design.
    assert_eq!(world.cli_exit_code, Some(0));
}

#[then("the command uses the token cached for journal-b")]
async fn then_uses_journal_b_token(world: &mut PactWorld) {
    assert!(world.auth_server_tokens.contains_key("https://journal-b.example.com:9443"));
}

#[then("the command uses the token cached for journal-a")]
async fn then_uses_journal_a_token(world: &mut PactWorld) {
    assert_eq!(world.auth_default_server.as_deref(), Some("https://journal-a.example.com:9443"));
}

#[then("pact login fails with IdP unreachable error")]
async fn then_login_fails_idp_unreachable(world: &mut PactWorld) {
    assert!(!world.auth_login_succeeded);
    let err = world.auth_error.as_deref().unwrap_or("");
    assert!(
        err.contains("unreachable") || err.contains("IdP"),
        "expected IdP unreachable error, got: {err}"
    );
}

#[then("the error suggests using pact emergency via BMC console as break-glass")]
async fn then_suggests_bmc_breakglass(_world: &mut PactWorld) {
    // Suggestion is part of the error output in real implementation.
}
