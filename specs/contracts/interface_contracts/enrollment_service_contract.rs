//! Contract tests for EnrollmentService (gRPC boundary).
//!
//! These tests verify the integration surface between:
//! - pact-agent (caller) ↔ pact-journal (EnrollmentService)
//! - pact-cli (caller) ↔ pact-journal (EnrollmentService)
//!
//! All tests reference their source contract and spec requirement.
//! Tests use stubs — they define WHAT the interface must do,
//! not HOW it does it internally.

// ---------------------------------------------------------------------------
// Enroll RPC contracts
// ---------------------------------------------------------------------------

/// Contract: enrollment-interfaces.md § Enroll
/// Spec: E1 — no connection without enrollment
/// If this test didn't exist: an agent with unknown hardware could get a signed cert.
#[test]
fn enroll_rejects_unknown_hardware_identity() {
    let registry = stub_empty_enrollment_registry();
    let ca = stub_ca_key_manager();

    let request = EnrollRequest {
        hardware_identity: HardwareIdentity {
            mac_addresses: vec!["ff:ff:ff:ff:ff:ff".into()],
            bmc_serial: "UNKNOWN".into(),
            tpm_ek_hash: None,
        },
        csr_pem: generate_test_csr(),
    };

    let result = enrollment_service_enroll(&registry, &ca, request);
    assert_matches!(result, Err(PactError::NodeNotEnrolled(_)));
}

/// Contract: enrollment-interfaces.md § Enroll
/// Spec: E7 — enrollment state governs CSR signing (Registered → Active)
/// If this test didn't exist: a Registered node might not get its CSR signed.
#[test]
fn enroll_signs_csr_for_registered_node() {
    let registry = stub_registry_with_node("compute-042", "aa:bb:cc:dd:ee:01", EnrollmentState::Registered);
    let ca = stub_ca_key_manager();

    let request = EnrollRequest {
        hardware_identity: hw_identity("aa:bb:cc:dd:ee:01", "SN12345"),
        csr_pem: generate_test_csr(),
    };

    let response = enrollment_service_enroll(&registry, &ca, request).unwrap();
    assert!(!response.signed_cert_pem.is_empty());
    assert_eq!(response.node_id, "compute-042");
    assert!(response.cert_serial.len() > 0);
    // State should now be Active
    assert_eq!(registry.get_state("compute-042"), EnrollmentState::Active);
}

/// Contract: enrollment-interfaces.md § Enroll
/// Spec: E7 — Active → Active rejected (once-Active rejection)
/// If this test didn't exist: a second caller could get a valid cert for the same node,
/// enabling concurrent impersonation.
#[test]
fn enroll_rejects_already_active_node() {
    let registry = stub_registry_with_node("compute-042", "aa:bb:cc:dd:ee:01", EnrollmentState::Active);
    let ca = stub_ca_key_manager();

    let request = EnrollRequest {
        hardware_identity: hw_identity("aa:bb:cc:dd:ee:01", "SN12345"),
        csr_pem: generate_test_csr(),
    };

    let result = enrollment_service_enroll(&registry, &ca, request);
    assert_matches!(result, Err(PactError::AlreadyActive(_)));
}

/// Contract: enrollment-interfaces.md § Enroll
/// Spec: E7 — Revoked nodes rejected
/// If this test didn't exist: a decommissioned node could re-enroll.
#[test]
fn enroll_rejects_revoked_node() {
    let registry = stub_registry_with_node("compute-042", "aa:bb:cc:dd:ee:01", EnrollmentState::Revoked);
    let ca = stub_ca_key_manager();

    let request = EnrollRequest {
        hardware_identity: hw_identity("aa:bb:cc:dd:ee:01", "SN12345"),
        csr_pem: generate_test_csr(),
    };

    let result = enrollment_service_enroll(&registry, &ca, request);
    assert_matches!(result, Err(PactError::NodeRevoked(_)));
}

/// Contract: enrollment-interfaces.md § Enroll
/// Spec: E7 — Inactive → Active succeeds (re-enrollment after heartbeat timeout)
/// If this test didn't exist: a node that rebooted couldn't reconnect after heartbeat timeout.
#[test]
fn enroll_succeeds_for_inactive_node() {
    let registry = stub_registry_with_node("compute-042", "aa:bb:cc:dd:ee:01", EnrollmentState::Inactive);
    let ca = stub_ca_key_manager();

    let request = EnrollRequest {
        hardware_identity: hw_identity("aa:bb:cc:dd:ee:01", "SN12345"),
        csr_pem: generate_test_csr(),
    };

    let response = enrollment_service_enroll(&registry, &ca, request).unwrap();
    assert!(!response.signed_cert_pem.is_empty());
    assert_eq!(registry.get_state("compute-042"), EnrollmentState::Active);
}

/// Contract: enrollment-interfaces.md § Enroll
/// Spec: ADR-008 — EnrollResponse includes vCluster assignment
/// If this test didn't exist: agent wouldn't know which vCluster to stream config for.
#[test]
fn enroll_response_includes_vcluster_assignment() {
    let registry = stub_registry_with_node_and_vcluster(
        "compute-042", "aa:bb:cc:dd:ee:01",
        EnrollmentState::Registered, Some("ml-training".into()),
    );
    let ca = stub_ca_key_manager();

    let request = EnrollRequest {
        hardware_identity: hw_identity("aa:bb:cc:dd:ee:01", "SN12345"),
        csr_pem: generate_test_csr(),
    };

    let response = enrollment_service_enroll(&registry, &ca, request).unwrap();
    assert_eq!(response.vcluster_id, Some("ml-training".into()));
}

/// Contract: enrollment-interfaces.md § Enroll
/// Spec: ADR-008 — unassigned node gets None for vCluster
/// If this test didn't exist: agent wouldn't know to enter maintenance mode.
#[test]
fn enroll_response_none_vcluster_for_unassigned() {
    let registry = stub_registry_with_node_and_vcluster(
        "compute-042", "aa:bb:cc:dd:ee:01",
        EnrollmentState::Registered, None,
    );
    let ca = stub_ca_key_manager();

    let request = EnrollRequest {
        hardware_identity: hw_identity("aa:bb:cc:dd:ee:01", "SN12345"),
        csr_pem: generate_test_csr(),
    };

    let response = enrollment_service_enroll(&registry, &ca, request).unwrap();
    assert_eq!(response.vcluster_id, None);
}

// ---------------------------------------------------------------------------
// RenewCert RPC contracts
// ---------------------------------------------------------------------------

/// Contract: enrollment-interfaces.md § RenewCert
/// Spec: E5 — cert renewal signs new CSR locally
/// If this test didn't exist: renewal could silently fail to produce a valid cert.
#[test]
fn renew_cert_signs_new_csr() {
    let registry = stub_registry_with_active_node("compute-042", "SERIAL-001");
    let ca = stub_ca_key_manager();

    let request = RenewCertRequest {
        node_id: "compute-042".into(),
        current_cert_serial: "SERIAL-001".into(),
        new_csr_pem: generate_test_csr(),
    };

    let response = enrollment_service_renew_cert(&registry, &ca, request).unwrap();
    assert!(!response.signed_cert_pem.is_empty());
    assert_ne!(response.cert_serial, "SERIAL-001"); // New serial
    assert!(response.not_after > Utc::now());
}

/// Contract: enrollment-interfaces.md § RenewCert
/// Spec: E5 — reject if current_cert_serial doesn't match
/// If this test didn't exist: an attacker with a stale cert serial could renew.
#[test]
fn renew_cert_rejects_mismatched_serial() {
    let registry = stub_registry_with_active_node("compute-042", "SERIAL-001");
    let ca = stub_ca_key_manager();

    let request = RenewCertRequest {
        node_id: "compute-042".into(),
        current_cert_serial: "WRONG-SERIAL".into(),
        new_csr_pem: generate_test_csr(),
    };

    let result = enrollment_service_renew_cert(&registry, &ca, request);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// RegisterNode RPC contracts
// ---------------------------------------------------------------------------

/// Contract: enrollment-interfaces.md § RegisterNode
/// Spec: E10 — only platform-admin can register
/// If this test didn't exist: ops users could add arbitrary nodes.
#[test]
fn register_node_rejects_non_admin() {
    let caller = Identity {
        principal: "user@example.com".into(),
        principal_type: PrincipalType::Human,
        role: "pact-ops-ml-training".into(),
    };

    let result = enrollment_service_register_node(&caller, register_request("compute-042", "aa:bb:cc:dd:ee:01"));
    assert_matches!(result, Err(PactError::Unauthorized { .. }));
}

/// Contract: enrollment-interfaces.md § RegisterNode
/// Spec: E2 — hardware identity uniqueness within domain
/// If this test didn't exist: two nodes with the same MAC could be enrolled.
#[test]
fn register_node_rejects_duplicate_hardware_identity() {
    let registry = stub_registry_with_node("compute-001", "aa:bb:cc:dd:ee:01", EnrollmentState::Registered);
    let caller = platform_admin();

    let result = enrollment_service_register_node_with_registry(
        &registry, &caller,
        register_request("compute-099", "aa:bb:cc:dd:ee:01"),
    );
    assert_matches!(result, Err(PactError::HardwareIdentityConflict { .. }));
}

/// Contract: enrollment-interfaces.md § RegisterNode
/// Spec: E2 — duplicate node_id rejected
/// If this test didn't exist: re-registering a node would silently overwrite.
#[test]
fn register_node_rejects_duplicate_node_id() {
    let registry = stub_registry_with_node("compute-042", "aa:bb:cc:dd:ee:01", EnrollmentState::Registered);
    let caller = platform_admin();

    let result = enrollment_service_register_node_with_registry(
        &registry, &caller,
        register_request("compute-042", "ff:ff:ff:ff:ff:ff"),
    );
    assert_matches!(result, Err(PactError::NodeAlreadyEnrolled(_)));
}

// ---------------------------------------------------------------------------
// DecommissionNode RPC contracts
// ---------------------------------------------------------------------------

/// Contract: enrollment-interfaces.md § DecommissionNode
/// Spec: ADR-008 — warns on active sessions without --force
/// If this test didn't exist: decommission could kill active admin sessions silently.
#[test]
fn decommission_warns_on_active_sessions_without_force() {
    let registry = stub_registry_with_active_node_and_sessions("compute-042", 2);
    let caller = platform_admin();

    let request = DecommissionRequest { node_id: "compute-042".into(), force: false };
    let result = enrollment_service_decommission(&registry, &caller, request);
    // Should return a warning, not proceed
    assert_matches!(result, Err(PactError::ActiveSessionsExist { count: 2, .. }));
}

/// Contract: enrollment-interfaces.md § DecommissionNode
/// Spec: E9 — decommission sets state to Revoked
/// If this test didn't exist: decommissioned nodes could still be Active.
#[test]
fn decommission_with_force_revokes_node() {
    let registry = stub_registry_with_active_node("compute-042", "SERIAL-001");
    let caller = platform_admin();

    let request = DecommissionRequest { node_id: "compute-042".into(), force: true };
    let response = enrollment_service_decommission(&registry, &caller, request).unwrap();

    assert_eq!(registry.get_state("compute-042"), EnrollmentState::Revoked);
}

// ---------------------------------------------------------------------------
// AssignNode / UnassignNode / MoveNode RPC contracts
// ---------------------------------------------------------------------------

/// Contract: enrollment-interfaces.md § AssignNode
/// Spec: E8 — vCluster assignment is independent of enrollment
/// If this test didn't exist: assignment might require a specific enrollment state.
#[test]
fn assign_node_works_for_any_non_revoked_state() {
    for state in [EnrollmentState::Registered, EnrollmentState::Active, EnrollmentState::Inactive] {
        let registry = stub_registry_with_node("compute-042", "aa:bb:cc:dd:ee:01", state);
        let caller = platform_admin();

        let result = enrollment_service_assign_node(
            &registry, &caller,
            "compute-042", "ml-training",
        );
        assert!(result.is_ok(), "assign should work for state {:?}", state);
        assert_eq!(registry.get_vcluster("compute-042"), Some("ml-training".into()));
    }
}

/// Contract: enrollment-interfaces.md § AssignNode
/// Spec: E10 — ops can assign to their own vCluster
/// If this test didn't exist: ops would need platform-admin for every assignment.
#[test]
fn assign_node_allowed_for_ops_own_vcluster() {
    let registry = stub_registry_with_active_node("compute-042", "SERIAL-001");
    let caller = Identity {
        principal: "user@example.com".into(),
        principal_type: PrincipalType::Human,
        role: "pact-ops-ml-training".into(),
    };

    let result = enrollment_service_assign_node(&registry, &caller, "compute-042", "ml-training");
    assert!(result.is_ok());
}

/// Contract: enrollment-interfaces.md § AssignNode
/// Spec: E10 — ops cannot assign to other vClusters
/// If this test didn't exist: ops for one vCluster could steal nodes from another.
#[test]
fn assign_node_denied_for_ops_other_vcluster() {
    let registry = stub_registry_with_active_node("compute-042", "SERIAL-001");
    let caller = Identity {
        principal: "user@example.com".into(),
        principal_type: PrincipalType::Human,
        role: "pact-ops-ml-training".into(),
    };

    let result = enrollment_service_assign_node(&registry, &caller, "compute-042", "regulated-bio");
    assert_matches!(result, Err(PactError::Unauthorized { .. }));
}

/// Contract: enrollment-interfaces.md § MoveNode
/// Spec: E8 — move does not affect certificate
/// If this test didn't exist: moving between vClusters might invalidate the cert.
#[test]
fn move_node_preserves_cert_serial() {
    let registry = stub_registry_with_active_node_in_vcluster("compute-042", "SERIAL-001", "ml-training");
    let caller = platform_admin();

    let serial_before = registry.get_cert_serial("compute-042");
    enrollment_service_move_node(&registry, &caller, "compute-042", "regulated-bio").unwrap();

    assert_eq!(registry.get_cert_serial("compute-042"), serial_before);
    assert_eq!(registry.get_vcluster("compute-042"), Some("regulated-bio".into()));
}
