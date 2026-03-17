//! Contract tests for authentication invariants.
//!
//! These test that the enforcement mechanisms identified in enforcement-map.md
//! actually prevent invariant violations.
//!
//! Source: specs/invariants.md § Authentication Invariants — hpc-auth crate (Auth1-Auth8)
//! Source: specs/invariants.md § Authentication Invariants — PACT-specific (PAuth1-PAuth5)

// ===========================================================================
// hpc-auth invariants (Auth1-Auth8)
// ===========================================================================

// ---------------------------------------------------------------------------
// Auth1: No unauthenticated commands
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § Auth1
/// Spec: invariants.md § Auth1 — authenticated commands fail without valid token
/// If this test didn't exist: commands could execute without checking token validity.
#[test]
fn auth1_no_unauthenticated_commands() {
    let auth_client = stub_auth_client_no_token();

    let authenticated_commands = vec!["status", "diff", "commit", "rollback", "exec", "shell"];
    for cmd in authenticated_commands {
        let result = auth_client.require_valid_token(cmd);
        assert_matches!(result, Err(AuthError::NoValidToken),
            "command {:?} must fail without a valid token", cmd);
    }
}

/// Contract: enforcement-map.md § Auth1
/// Spec: invariants.md § Auth1 — login, logout, version, --help are exempt
/// If this test didn't exist: users could not login when they have no token.
#[test]
fn auth1_exempt_commands() {
    let auth_client = stub_auth_client_no_token();

    let exempt_commands = vec!["login", "logout", "version", "--help"];
    for cmd in exempt_commands {
        let result = auth_client.require_valid_token(cmd);
        assert!(result.is_ok(),
            "command {:?} must be exempt from token requirement", cmd);
    }
}

// ---------------------------------------------------------------------------
// Auth2: Fail closed on cache corruption
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § Auth2
/// Spec: invariants.md § Auth2 — corrupted JSON cache is rejected
/// If this test didn't exist: malformed cache could produce undefined behavior.
#[test]
fn auth2_corrupted_cache_rejected() {
    let corrupted_payloads = vec![
        "",                           // empty
        "{",                          // truncated JSON
        "not json at all",            // garbage
        "{\"access_token\": null}",   // null token
        "\x00\x01\x02\x03",          // binary garbage
    ];

    for payload in corrupted_payloads {
        let cache = stub_token_cache_with_content(payload);
        let result = cache.load();
        assert_matches!(result, Err(AuthError::CacheCorrupted(_)),
            "payload {:?} must yield CacheCorrupted", payload);
    }
}

// ---------------------------------------------------------------------------
// Auth3: Concurrent refresh safety
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § Auth3
/// Spec: invariants.md § Auth3 — multiple refreshes don't corrupt state
/// If this test didn't exist: concurrent CLI invocations could corrupt the token cache.
#[test]
fn auth3_concurrent_refresh_safe() {
    let cache = stub_shared_token_cache();
    let idp = stub_idp_server();

    // Simulate 10 concurrent refresh attempts
    let handles: Vec<_> = (0..10).map(|i| {
        let cache = cache.clone();
        let idp = idp.clone();
        std::thread::spawn(move || {
            cache.refresh_token(&idp, &format!("refresh-token-{}", i))
        })
    }).collect();

    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    // All must succeed (idempotent at IdP)
    for (i, result) in results.iter().enumerate() {
        assert!(result.is_ok(), "refresh {} must succeed", i);
    }

    // Cache must be readable and valid after concurrent writes
    let final_token = cache.load().unwrap();
    assert!(!final_token.access_token.is_empty(),
        "cache must contain a valid token after concurrent refreshes");
}

// ---------------------------------------------------------------------------
// Auth4: Logout always clears local state
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § Auth4
/// Spec: invariants.md § Auth4 — cache deleted before IdP revocation attempt
/// If this test didn't exist: failed IdP revocation could leave tokens on disk.
#[test]
fn auth4_logout_deletes_before_revoke() {
    let cache = stub_token_cache_with_valid_token();
    let idp = stub_idp_server_that_fails_revocation();
    let event_log = stub_event_log();

    let result = auth_client_logout(&cache, &idp, &event_log);

    // Logout may report IdP revocation failure, but that's a warning
    // The critical assertion: cache is deleted regardless
    assert!(!cache.exists(), "token cache must be deleted even if IdP revocation fails");

    let events = event_log.events();
    let delete_idx = events.iter().position(|e| e == "cache_deleted").unwrap();
    let revoke_idx = events.iter().position(|e| e == "idp_revoke_attempted").unwrap();
    assert!(delete_idx < revoke_idx,
        "cache deletion (idx {}) must happen before IdP revocation attempt (idx {})",
        delete_idx, revoke_idx);
}

// ---------------------------------------------------------------------------
// Auth5: Cache file permissions
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § Auth5
/// Spec: invariants.md § Auth5 — 0644 cache rejected in strict mode
/// If this test didn't exist: world-readable token caches could be used.
#[test]
fn auth5_strict_mode_rejects_wrong_perms() {
    let cache = stub_token_cache_with_permissions(0o644);
    let auth_client = stub_auth_client(PermissionMode::Strict, &cache);

    let result = auth_client.load_token();
    assert_matches!(result, Err(AuthError::InsecurePermissions { mode: 0o644, .. }),
        "strict mode must reject 0644 permissions");
}

/// Contract: enforcement-map.md § Auth5
/// Spec: invariants.md § Auth5 — wrong perms warned + fixed in lenient mode
/// If this test didn't exist: lenient mode could silently ignore insecure perms.
#[test]
fn auth5_lenient_mode_fixes_perms() {
    let cache = stub_token_cache_with_permissions(0o644);
    let warnings = stub_warning_sink();
    let auth_client = stub_auth_client_with_warnings(PermissionMode::Lenient, &cache, &warnings);

    let result = auth_client.load_token();
    assert!(result.is_ok(), "lenient mode must proceed after fixing permissions");
    assert_eq!(cache.permissions(), 0o600, "lenient mode must fix permissions to 0600");
    assert!(!warnings.is_empty(), "lenient mode must emit a warning about permissions");
}

// ---------------------------------------------------------------------------
// Auth6: Per-server token isolation
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § Auth6
/// Spec: invariants.md § Auth6 — tokens for server A don't leak to server B
/// If this test didn't exist: multi-server users could get cross-contamination.
#[test]
fn auth6_per_server_isolation() {
    let cache_dir = stub_cache_directory();
    let server_a = "https://pact-a.example.com:9443";
    let server_b = "https://pact-b.example.com:9443";

    // Store token for server A
    let cache_a = TokenCache::for_server(&cache_dir, server_a);
    cache_a.store(&valid_token("token-for-a")).unwrap();

    // Load from server B's perspective — must not see server A's token
    let cache_b = TokenCache::for_server(&cache_dir, server_b);
    let result = cache_b.load();
    assert_matches!(result, Err(AuthError::NoValidToken),
        "server B must not see server A's token");

    // Server A's token must still be accessible
    let loaded = cache_a.load().unwrap();
    assert_eq!(loaded.access_token, "token-for-a");
}

// ---------------------------------------------------------------------------
// Auth7: Refresh tokens never logged
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § Auth7
/// Spec: invariants.md § Auth7 — Display/Debug impl redacts refresh_token
/// If this test didn't exist: refresh tokens could leak into logs or diagnostics.
#[test]
fn auth7_refresh_token_never_logged() {
    let token = TokenSet {
        access_token: "access-visible".into(),
        refresh_token: Some("super-secret-refresh".into()),
        client_secret: Some("super-secret-client".into()),
        expires_at: Utc::now() + Duration::hours(1),
    };

    let debug_output = format!("{:?}", token);
    let display_output = format!("{}", token);

    assert!(!debug_output.contains("super-secret-refresh"),
        "Debug must redact refresh_token");
    assert!(!debug_output.contains("super-secret-client"),
        "Debug must redact client_secret");
    assert!(!display_output.contains("super-secret-refresh"),
        "Display must redact refresh_token");
    assert!(!display_output.contains("super-secret-client"),
        "Display must redact client_secret");

    // access_token is not secret (it's sent in headers anyway)
    assert!(debug_output.contains("access-visible") || debug_output.contains("[REDACTED]"),
        "Debug output must be well-formed");
}

// ---------------------------------------------------------------------------
// Auth8: Cascading flow fallback
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § Auth8
/// Spec: invariants.md § Auth8 — PKCE -> Confidential -> DeviceCode -> ManualPaste
/// If this test didn't exist: flow selection could be wrong or hardcoded.
#[test]
fn auth8_cascade_fallback() {
    // IdP that supports all flows
    let full_idp = stub_idp_discovery(vec![
        FlowCapability::AuthCodePKCE,
        FlowCapability::ConfidentialClient,
        FlowCapability::DeviceCode,
    ]);
    assert_eq!(select_auth_flow(&full_idp), AuthFlow::AuthCodePKCE,
        "must prefer PKCE when available");

    // IdP without PKCE
    let no_pkce = stub_idp_discovery(vec![
        FlowCapability::ConfidentialClient,
        FlowCapability::DeviceCode,
    ]);
    assert_eq!(select_auth_flow(&no_pkce), AuthFlow::ConfidentialClient,
        "must fall back to Confidential when PKCE unavailable");

    // IdP with only device code
    let device_only = stub_idp_discovery(vec![
        FlowCapability::DeviceCode,
    ]);
    assert_eq!(select_auth_flow(&device_only), AuthFlow::DeviceCode,
        "must fall back to DeviceCode when Confidential unavailable");

    // IdP with no standard flows
    let bare_idp = stub_idp_discovery(vec![]);
    assert_eq!(select_auth_flow(&bare_idp), AuthFlow::ManualPaste,
        "must fall back to ManualPaste when no standard flows available");
}

// ===========================================================================
// PACT-specific authentication invariants (PAuth1-PAuth5)
// ===========================================================================

// ---------------------------------------------------------------------------
// PAuth1: Strict permission mode
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § PAuth1
/// Spec: invariants.md § PAuth1 — pact CLI constructs AuthClient with Strict
/// If this test didn't exist: pact CLI could default to lenient, ignoring insecure caches.
#[test]
fn pauth1_pact_uses_strict_mode() {
    let pact_config = default_pact_cli_config();
    let auth_client = AuthClient::from_config(&pact_config);

    assert_eq!(auth_client.permission_mode(), PermissionMode::Strict,
        "pact CLI must default to Strict permission mode");
}

// ---------------------------------------------------------------------------
// PAuth2: Emergency mode requires human identity
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § PAuth2
/// Spec: invariants.md § PAuth2 — service/AI principal denied emergency
/// If this test didn't exist: automated systems could enter emergency mode.
#[test]
fn pauth2_emergency_requires_human() {
    let non_human_principals = vec![
        Identity {
            principal: "agent@node.local".into(),
            principal_type: PrincipalType::Service,
            role: "pact-service-agent".into(),
        },
        Identity {
            principal: "claude@mcp.local".into(),
            principal_type: PrincipalType::Service,
            role: "pact-service-ai".into(),
        },
    ];

    for identity in non_human_principals {
        let result = validate_emergency_access(&identity);
        assert_matches!(result, Err(PactError::EmergencyRequiresHuman),
            "principal {:?} must be denied emergency access", identity.principal);
    }

    // Human principal must be allowed (assuming correct role)
    let human = Identity {
        principal: "admin@example.com".into(),
        principal_type: PrincipalType::Human,
        role: "pact-platform-admin".into(),
    };
    let result = validate_emergency_access(&human);
    assert!(result.is_ok(), "human principal must be allowed emergency access");
}

// ---------------------------------------------------------------------------
// PAuth3: Auth discovery endpoint is public
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § PAuth3
/// Spec: invariants.md § PAuth3 — /auth/discovery requires no authentication
/// If this test didn't exist: unauthenticated clients could not discover how to login.
#[test]
fn pauth3_discovery_endpoint_unauthenticated() {
    let journal = stub_journal_server();

    // No token, no auth headers
    let request = HttpRequest {
        method: "GET".into(),
        path: "/auth/discovery".into(),
        headers: HashMap::new(),
    };

    let response = journal.handle_request(request);
    assert_eq!(response.status, 200,
        "/auth/discovery must succeed without authentication");

    let discovery: AuthDiscovery = serde_json::from_str(&response.body).unwrap();
    assert!(!discovery.idp_url.is_empty(), "discovery must include IdP URL");
    assert!(!discovery.client_id.is_empty(), "discovery must include public client ID");
}

// ---------------------------------------------------------------------------
// PAuth4: Break-glass is BMC console
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § PAuth4
/// Spec: invariants.md § PAuth4 — IdP down error suggests BMC console
/// If this test didn't exist: admins locked out by IdP failure get no guidance.
#[test]
fn pauth4_break_glass_is_bmc() {
    let auth_client = stub_auth_client_with_unreachable_idp();

    let result = auth_client.login();
    assert_matches!(result, Err(AuthError::IdpUnreachable { suggestion, .. }) => {
        assert!(suggestion.to_lowercase().contains("bmc"),
            "IdP-down error must suggest BMC console as break-glass path");
    });
}

// ---------------------------------------------------------------------------
// PAuth5: Two-person approval requires distinct identities
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § PAuth5
/// Spec: invariants.md § PAuth5 — same-identity approval rejected
/// If this test didn't exist: one person could approve their own requests.
#[test]
fn pauth5_two_person_distinct_identities() {
    let requester = test_identity("ops@example.com", "pact-ops-regulated-bio");

    // Same principal, fresh token (different session but same identity)
    let approver_same = test_identity("ops@example.com", "pact-ops-regulated-bio");

    let pending = PendingApproval {
        id: "approval-003".into(),
        requested_by: requester.clone(),
        operation: Operation::CommitConfig { vcluster_id: "regulated-bio".into() },
        expires_at: Utc::now() + Duration::minutes(30),
    };

    let result = validate_two_person_approval(&approver_same, &pending);
    assert_matches!(result, Err(PactError::SelfApprovalDenied),
        "same-identity approval must be rejected regardless of token freshness");

    // Different principal must succeed
    let approver_different = test_identity("admin@example.com", "pact-ops-regulated-bio");
    let result = validate_two_person_approval(&approver_different, &pending);
    assert!(result.is_ok(), "different-identity approval must succeed");
}
