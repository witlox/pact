//! Contract tests for enrollment failure mode degradation.
//!
//! These test that failure modes F18, F19, F20 degrade as specified.
//!
//! Source: specs/failure-modes.md § F18-F20

// ---------------------------------------------------------------------------
// F18: Vault unreachable for journal CA key rotation
// ---------------------------------------------------------------------------

/// Contract: failure-modes.md § F18
/// Spec: current CA key continues signing when Vault is unreachable
/// If this test didn't exist: CA key rotation failure could block all enrollments.
#[test]
fn f18_ca_signing_continues_when_vault_unreachable() {
    let ca = stub_ca_key_manager_with_valid_key();
    let vault = stub_vault_crl_client_unreachable();

    // CA key is valid — signing should work regardless of Vault state
    let csr = generate_test_csr();
    let result = ca.sign_csr(&csr, "compute-042", "site-alpha", Duration::days(3));
    assert!(result.is_ok(), "signing must work with valid CA key even when Vault is unreachable");
}

/// Contract: failure-modes.md § F18
/// Spec: CA key approaching expiry triggers warning
/// If this test didn't exist: CA cert could expire silently, breaking all future enrollments.
#[test]
fn f18_ca_key_approaching_expiry_detected() {
    let ca = stub_ca_key_manager_expiring_in(Duration::days(5));
    assert!(ca.needs_rotation(), "CA cert expiring within 7 days should trigger rotation flag");

    let ca_fresh = stub_ca_key_manager_expiring_in(Duration::days(30));
    assert!(!ca_fresh.needs_rotation(), "CA cert with 30 days remaining should not need rotation");
}

// ---------------------------------------------------------------------------
// F19: Journal unreachable during agent certificate renewal
// ---------------------------------------------------------------------------

/// Contract: failure-modes.md § F19
/// Spec: active channel continues when renewal fails
/// If this test didn't exist: a renewal failure could terminate the active connection.
#[test]
fn f19_active_channel_survives_renewal_failure() {
    let client = stub_dual_channel_client_with_cert_expiring_in(Duration::hours(20));

    // Simulate renewal failure (journal unreachable)
    let result = client.rotate_with_unreachable_journal();
    assert!(result.is_err());

    // Active channel must still be functional
    assert!(client.active_channel_healthy(), "active channel must survive renewal failure");
    assert!(client.needs_renewal(), "should still flag renewal needed");
}

/// Contract: failure-modes.md § F19
/// Spec: agent enters degraded mode only after cert actually expires
/// If this test didn't exist: renewal failure might trigger premature degraded mode.
#[test]
fn f19_degraded_mode_only_on_actual_expiry() {
    let client = stub_dual_channel_client_with_cert_expiring_in(Duration::hours(20));

    // Renewal failed but cert still valid
    let _ = client.rotate_with_unreachable_journal();
    assert!(!client.is_degraded(), "should not be degraded while cert is still valid");

    // Simulate cert expiry
    let client_expired = stub_dual_channel_client_with_expired_cert();
    assert!(client_expired.is_degraded(), "should be degraded after cert expires");
}

// ---------------------------------------------------------------------------
// F20: Hardware identity mismatch at boot
// ---------------------------------------------------------------------------

/// Contract: failure-modes.md § F20
/// Spec: agent retries enrollment periodically
/// If this test didn't exist: a boot with wrong hardware could cause agent to exit permanently.
#[test]
fn f20_enrollment_rejection_is_retryable() {
    let result = enrollment_service_enroll(
        &stub_empty_enrollment_registry(),
        &stub_ca_key_manager(),
        EnrollRequest {
            hardware_identity: hw_identity("ff:ff:ff:ff:ff:ff", "UNKNOWN"),
            csr_pem: generate_test_csr(),
        },
    );

    match result {
        Err(PactError::NodeNotEnrolled(_)) => {
            // This error is retryable — agent should retry on interval
            // The error type must be distinguishable from permanent errors
        }
        other => panic!("expected NodeNotEnrolled, got {:?}", other),
    }
}

/// Contract: failure-modes.md § F20
/// Spec: NODE_NOT_ENROLLED is distinct from NODE_REVOKED
/// If this test didn't exist: agent couldn't distinguish "never enrolled" from "decommissioned".
#[test]
fn f20_not_enrolled_distinct_from_revoked() {
    let ca = stub_ca_key_manager();

    // Not enrolled at all
    let result1 = enrollment_service_enroll(
        &stub_empty_enrollment_registry(), &ca,
        EnrollRequest {
            hardware_identity: hw_identity("aa:bb:cc:dd:ee:01", "SN12345"),
            csr_pem: generate_test_csr(),
        },
    );

    // Enrolled but revoked
    let result2 = enrollment_service_enroll(
        &stub_registry_with_node("compute-042", "aa:bb:cc:dd:ee:01", EnrollmentState::Revoked), &ca,
        EnrollRequest {
            hardware_identity: hw_identity("aa:bb:cc:dd:ee:01", "SN12345"),
            csr_pem: generate_test_csr(),
        },
    );

    // Must be different error variants
    assert_matches!(result1, Err(PactError::NodeNotEnrolled(_)));
    assert_matches!(result2, Err(PactError::NodeRevoked(_)));
}
