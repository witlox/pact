//! Contract tests for cross-module enrollment invariants.
//!
//! These test that the enforcement mechanisms identified in enforcement-map.md
//! actually prevent invariant violations.
//!
//! Source: specs/invariants.md § Enrollment & Certificate Invariants (E1-E10)

// ---------------------------------------------------------------------------
// E1: No connection without enrollment
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § E1
/// Spec: invariants.md § E1 — EnrollmentService.Enroll rejects unknown hardware
/// If this test didn't exist: an unenrolled node could get a cert and connect.
#[test]
fn e1_unenrolled_node_cannot_get_certificate() {
    let empty_registry = stub_empty_enrollment_registry();
    let ca = stub_ca_key_manager();

    // Try every conceivable hardware identity against an empty registry
    for mac in ["00:00:00:00:00:01", "ff:ff:ff:ff:ff:ff", "aa:bb:cc:dd:ee:ff"] {
        let request = EnrollRequest {
            hardware_identity: hw_identity(mac, "ANY-SERIAL"),
            csr_pem: generate_test_csr(),
        };
        let result = enrollment_service_enroll(&empty_registry, &ca, request);
        assert_matches!(result, Err(PactError::NodeNotEnrolled(_)),
            "MAC {} should be rejected", mac);
    }
}

// ---------------------------------------------------------------------------
// E2: Hardware identity uniqueness per domain
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § E2
/// Spec: invariants.md § E2 — duplicate MAC+BMC rejected within domain
/// If this test didn't exist: two nodes with same hardware could be enrolled,
/// causing identity confusion at boot.
#[test]
fn e2_duplicate_hardware_identity_rejected() {
    let registry = stub_registry_with_node("node-001", "aa:bb:cc:dd:ee:01", EnrollmentState::Registered);
    let caller = platform_admin();

    // Same MAC, different node_id
    let result = enrollment_service_register_node_with_registry(
        &registry, &caller,
        register_request("node-002", "aa:bb:cc:dd:ee:01"),
    );
    assert_matches!(result, Err(PactError::HardwareIdentityConflict { .. }));
}

// ---------------------------------------------------------------------------
// E4: CSR model — no private keys in journal
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § E4
/// Spec: invariants.md § E4 — journal never touches private keys
/// If this test didn't exist: the CaKeyManager could accidentally store private keys.
#[test]
fn e4_ca_key_manager_sign_csr_returns_only_cert() {
    let ca = stub_ca_key_manager();
    let csr = generate_test_csr();

    let signed = ca.sign_csr(&csr, "compute-042", "site-alpha", Duration::days(3)).unwrap();

    // SignedCert has cert_pem, serial, not_before, not_after — NO key_pem
    assert!(signed.cert_pem.contains("BEGIN CERTIFICATE"));
    assert!(!signed.cert_pem.contains("PRIVATE KEY"));
    assert!(!signed.serial.is_empty());
}

/// Contract: enforcement-map.md § E4
/// Spec: invariants.md § E4 — Raft state does not contain private key material
/// If this test didn't exist: private keys could leak into snapshots/WAL.
#[test]
fn e4_enrollment_record_in_raft_has_no_key_material() {
    let enrollment = NodeEnrollment {
        node_id: "compute-042".into(),
        hardware_identity: hw_identity("aa:bb:cc:dd:ee:01", "SN12345"),
        domain_id: "site-alpha".into(),
        enrolled_by: test_identity("admin@example.com", "pact-platform-admin"),
        enrolled_at: Utc::now(),
        state: EnrollmentState::Active,
        vcluster_id: None,
        assigned_by: None,
        assigned_at: None,
        cert_serial: Some("SERIAL-001".into()),
        cert_not_after: Some(Utc::now() + Duration::days(3)),
        last_seen: Some(Utc::now()),
    };

    let serialized = serde_json::to_string(&enrollment).unwrap();
    assert!(!serialized.contains("PRIVATE KEY"), "enrollment record must not contain private key");
    assert!(!serialized.contains("key_pem"), "enrollment record must not have key_pem field");
}

// ---------------------------------------------------------------------------
// E7: Enrollment state governs CSR signing
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § E7
/// Spec: invariants.md § E7 — state machine transitions tested exhaustively
/// If this test didn't exist: a state could be incorrectly allowed/denied.
#[test]
fn e7_enrollment_state_machine_transitions() {
    let ca = stub_ca_key_manager();
    let csr = generate_test_csr();

    struct TestCase {
        initial_state: EnrollmentState,
        expect_success: bool,
        expected_error: Option<&'static str>,
    }

    let cases = vec![
        TestCase { initial_state: EnrollmentState::Registered, expect_success: true, expected_error: None },
        TestCase { initial_state: EnrollmentState::Inactive, expect_success: true, expected_error: None },
        TestCase { initial_state: EnrollmentState::Active, expect_success: false, expected_error: Some("AlreadyActive") },
        TestCase { initial_state: EnrollmentState::Revoked, expect_success: false, expected_error: Some("NodeRevoked") },
    ];

    for case in cases {
        let registry = stub_registry_with_node("compute-042", "aa:bb:cc:dd:ee:01", case.initial_state.clone());
        let request = EnrollRequest {
            hardware_identity: hw_identity("aa:bb:cc:dd:ee:01", "SN12345"),
            csr_pem: csr.clone(),
        };

        let result = enrollment_service_enroll(&registry, &ca, request);
        assert_eq!(result.is_ok(), case.expect_success,
            "state {:?} should {} enrollment",
            case.initial_state, if case.expect_success { "allow" } else { "deny" });
    }
}

// ---------------------------------------------------------------------------
// E8: vCluster assignment independent of enrollment
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § E8
/// Spec: invariants.md § E8 — cert CN has no vCluster
/// If this test didn't exist: cert could contain vCluster, breaking vCluster moves.
#[test]
fn e8_cert_cn_contains_no_vcluster() {
    let ca = stub_ca_key_manager();
    let csr = generate_test_csr();

    let signed = ca.sign_csr(&csr, "compute-042", "site-alpha", Duration::days(3)).unwrap();

    // Parse cert and check CN
    let cn = extract_cn_from_cert(&signed.cert_pem);
    assert_eq!(cn, "pact-service-agent/compute-042@site-alpha");
    assert!(!cn.contains("ml-training"), "cert CN must not contain vCluster");
}

// ---------------------------------------------------------------------------
// E9: Decommission revokes certificate
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § E9
/// Spec: invariants.md § E9 — decommission sets Revoked + publishes to CRL
/// If this test didn't exist: decommissioned nodes could still use their certs.
#[test]
fn e9_decommission_transitions_to_revoked_and_revokes_cert() {
    let registry = stub_registry_with_active_node("compute-042", "SERIAL-001");
    let crl_client = stub_vault_crl_client();
    let caller = platform_admin();

    let request = DecommissionRequest { node_id: "compute-042".into(), force: true };
    enrollment_service_decommission_with_crl(&registry, &crl_client, &caller, request).unwrap();

    assert_eq!(registry.get_state("compute-042"), EnrollmentState::Revoked);
    assert!(crl_client.was_revoked("SERIAL-001"), "cert serial should be published to CRL");
}

// ---------------------------------------------------------------------------
// E10: Only platform-admin can enroll/decommission
// ---------------------------------------------------------------------------

/// Contract: enforcement-map.md § E10
/// Spec: invariants.md § E10 — RBAC enforcement for enrollment operations
/// If this test didn't exist: non-admins could register/decommission nodes.
#[test]
fn e10_rbac_enforcement_for_enrollment_operations() {
    let non_admin_roles = vec![
        "pact-ops-ml-training",
        "pact-viewer-ml-training",
        "pact-regulated-ml-training",
        "pact-service-agent",
        "pact-service-ai",
    ];

    for role in non_admin_roles {
        let caller = Identity {
            principal: "user@example.com".into(),
            principal_type: PrincipalType::Human,
            role: role.into(),
        };

        // RegisterNode
        let result = enrollment_service_register_node(&caller, register_request("node-new", "ff:ff:ff:ff:ff:ff"));
        assert_matches!(result, Err(PactError::Unauthorized { .. }),
            "role {} should not be able to register nodes", role);

        // DecommissionNode
        let registry = stub_registry_with_active_node("compute-042", "SERIAL-001");
        let result = enrollment_service_decommission(
            &registry, &caller,
            DecommissionRequest { node_id: "compute-042".into(), force: true },
        );
        assert_matches!(result, Err(PactError::Unauthorized { .. }),
            "role {} should not be able to decommission nodes", role);
    }
}
