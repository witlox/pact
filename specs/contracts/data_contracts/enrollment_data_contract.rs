//! Contract tests for enrollment data models (serialization + validation).
//!
//! These tests verify:
//! - Round-trip serialization of enrollment types
//! - Validation constraints from the spec
//! - Cross-module type compatibility
//!
//! Source: specs/architecture/data-models/shared-kernel.md § Node Enrollment

// ---------------------------------------------------------------------------
// Serialization round-trip contracts
// ---------------------------------------------------------------------------

/// Contract: shared-kernel.md § NodeEnrollment
/// Spec: data model must serialize/deserialize without loss
/// If this test didn't exist: a field could be silently dropped during serde.
#[test]
fn node_enrollment_round_trip() {
    let enrollment = NodeEnrollment {
        node_id: "compute-042".into(),
        hardware_identity: HardwareIdentity {
            mac_addresses: vec!["aa:bb:cc:dd:ee:01".into()],
            bmc_serial: "SN12345".into(),
            tpm_ek_hash: Some("sha256:abcdef".into()),
        },
        domain_id: "site-alpha".into(),
        enrolled_by: test_identity("admin@example.com", "pact-platform-admin"),
        enrolled_at: Utc::now(),
        state: EnrollmentState::Active,
        vcluster_id: Some("ml-training".into()),
        assigned_by: Some(test_identity("admin@example.com", "pact-platform-admin")),
        assigned_at: Some(Utc::now()),
        cert_serial: Some("SERIAL-001".into()),
        cert_not_after: Some(Utc::now() + Duration::days(3)),
        last_seen: Some(Utc::now()),
    };

    let json = serde_json::to_string(&enrollment).unwrap();
    let deserialized: NodeEnrollment = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.node_id, enrollment.node_id);
    assert_eq!(deserialized.hardware_identity.mac_addresses, enrollment.hardware_identity.mac_addresses);
    assert_eq!(deserialized.hardware_identity.bmc_serial, enrollment.hardware_identity.bmc_serial);
    assert_eq!(deserialized.hardware_identity.tpm_ek_hash, enrollment.hardware_identity.tpm_ek_hash);
    assert_eq!(deserialized.domain_id, enrollment.domain_id);
    assert_eq!(deserialized.state, enrollment.state);
    assert_eq!(deserialized.vcluster_id, enrollment.vcluster_id);
    assert_eq!(deserialized.cert_serial, enrollment.cert_serial);
}

/// Contract: shared-kernel.md § HardwareIdentity
/// Spec: tpm_ek_hash is optional — round-trip must preserve None
/// If this test didn't exist: None could become Some("") or vice versa.
#[test]
fn hardware_identity_optional_tpm_round_trip() {
    let hw = HardwareIdentity {
        mac_addresses: vec!["aa:bb:cc:dd:ee:01".into()],
        bmc_serial: "SN12345".into(),
        tpm_ek_hash: None,
    };

    let json = serde_json::to_string(&hw).unwrap();
    let deserialized: HardwareIdentity = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.tpm_ek_hash, None);
}

/// Contract: shared-kernel.md § SignedCert
/// Spec: ADR-008 — no key_pem field exists
/// If this test didn't exist: someone could add a key_pem field and store private keys.
#[test]
fn signed_cert_has_no_private_key_field() {
    let cert = SignedCert {
        cert_pem: "-----BEGIN CERTIFICATE-----\ntest\n-----END CERTIFICATE-----".into(),
        serial: "SERIAL-001".into(),
        not_before: Utc::now(),
        not_after: Utc::now() + Duration::days(3),
    };

    let json = serde_json::to_string(&cert).unwrap();
    // The serialized form must NOT contain "key" or "private"
    assert!(!json.contains("private"), "SignedCert must not contain private key data");
    assert!(!json.contains("key_pem"), "SignedCert must not have key_pem field");
}

/// Contract: shared-kernel.md § EnrollmentState
/// Spec: domain-model.md EnrollmentState state machine — all 4 variants exist
/// If this test didn't exist: a variant could be missing, breaking state transitions.
#[test]
fn enrollment_state_has_all_variants() {
    let states = vec![
        EnrollmentState::Registered,
        EnrollmentState::Active,
        EnrollmentState::Inactive,
        EnrollmentState::Revoked,
    ];

    for state in &states {
        let json = serde_json::to_string(state).unwrap();
        let deserialized: EnrollmentState = serde_json::from_str(&json).unwrap();
        assert_eq!(&deserialized, state);
    }
}

// ---------------------------------------------------------------------------
// Validation contracts
// ---------------------------------------------------------------------------

/// Contract: shared-kernel.md § NodeEnrollment
/// Spec: E2 — hardware_identity.mac_addresses must not be empty
/// If this test didn't exist: a node with no MACs could be enrolled, breaking identity.
#[test]
fn hardware_identity_requires_at_least_one_mac() {
    let hw = HardwareIdentity {
        mac_addresses: vec![],
        bmc_serial: "SN12345".into(),
        tpm_ek_hash: None,
    };

    let result = validate_hardware_identity(&hw);
    assert!(result.is_err(), "empty mac_addresses should be rejected");
}

/// Contract: shared-kernel.md § NodeEnrollment
/// Spec: E2 — hardware_identity.bmc_serial must not be empty
/// If this test didn't exist: a node with no BMC serial could be enrolled.
#[test]
fn hardware_identity_requires_bmc_serial() {
    let hw = HardwareIdentity {
        mac_addresses: vec!["aa:bb:cc:dd:ee:01".into()],
        bmc_serial: "".into(),
        tpm_ek_hash: None,
    };

    let result = validate_hardware_identity(&hw);
    assert!(result.is_err(), "empty bmc_serial should be rejected");
}

/// Contract: shared-kernel.md § NodeEnrollment
/// Spec: E8 — vcluster_id can be None (maintenance mode)
/// If this test didn't exist: None vcluster could cause panics downstream.
#[test]
fn enrollment_with_none_vcluster_is_valid() {
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

    let result = validate_enrollment(&enrollment);
    assert!(result.is_ok());
}
