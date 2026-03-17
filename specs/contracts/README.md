# Contract Tests — Node Enrollment Feature (ADR-008)

Contract tests verify the integration surfaces between modules for the enrollment feature. They test BOUNDARIES, not internals.

## How to run

These are Rust-flavored test specifications. They become executable when the enrollment types and interfaces are implemented in `pact-common`, `pact-journal`, and `pact-agent`.

```bash
cargo test -p pact-acceptance --test enrollment_contracts
```

## Test files

| File | What it tests | Count |
|---|---|---|
| `interface_contracts/enrollment_service_contract.rs` | EnrollmentService gRPC boundary (Enroll, RenewCert, RegisterNode, DecommissionNode, AssignNode, MoveNode) | 16 |
| `data_contracts/enrollment_data_contract.rs` | Serialization round-trips, validation constraints, field presence/absence | 7 |
| `invariant_contracts/enrollment_invariant_contract.rs` | E1, E2, E4, E7, E8, E9, E10 enforcement mechanisms | 8 |
| `failure_contracts/enrollment_failure_contract.rs` | F18, F19, F20 degradation behavior | 6 |

**Total: 37 contract tests**

## Coverage Matrix

| Contract Source | Contract Description | Test File | Status |
|---|---|---|---|
| enrollment-interfaces.md § Enroll | Reject unknown hardware identity | enrollment_service_contract::enroll_rejects_unknown_hardware_identity | 🔲 |
| enrollment-interfaces.md § Enroll | Sign CSR for Registered node | enrollment_service_contract::enroll_signs_csr_for_registered_node | 🔲 |
| enrollment-interfaces.md § Enroll | Reject Active node (once-Active) | enrollment_service_contract::enroll_rejects_already_active_node | 🔲 |
| enrollment-interfaces.md § Enroll | Reject Revoked node | enrollment_service_contract::enroll_rejects_revoked_node | 🔲 |
| enrollment-interfaces.md § Enroll | Sign CSR for Inactive node | enrollment_service_contract::enroll_succeeds_for_inactive_node | 🔲 |
| enrollment-interfaces.md § Enroll | Response includes vCluster | enrollment_service_contract::enroll_response_includes_vcluster_assignment | 🔲 |
| enrollment-interfaces.md § Enroll | Response None for unassigned | enrollment_service_contract::enroll_response_none_vcluster_for_unassigned | 🔲 |
| enrollment-interfaces.md § RenewCert | Sign new CSR on renewal | enrollment_service_contract::renew_cert_signs_new_csr | 🔲 |
| enrollment-interfaces.md § RenewCert | Reject mismatched serial | enrollment_service_contract::renew_cert_rejects_mismatched_serial | 🔲 |
| enrollment-interfaces.md § RegisterNode | Reject non-admin | enrollment_service_contract::register_node_rejects_non_admin | 🔲 |
| enrollment-interfaces.md § RegisterNode | Reject duplicate hardware | enrollment_service_contract::register_node_rejects_duplicate_hardware_identity | 🔲 |
| enrollment-interfaces.md § RegisterNode | Reject duplicate node_id | enrollment_service_contract::register_node_rejects_duplicate_node_id | 🔲 |
| enrollment-interfaces.md § DecommissionNode | Warn on active sessions | enrollment_service_contract::decommission_warns_on_active_sessions_without_force | 🔲 |
| enrollment-interfaces.md § DecommissionNode | Force revokes node | enrollment_service_contract::decommission_with_force_revokes_node | 🔲 |
| enrollment-interfaces.md § AssignNode | Works for non-revoked states | enrollment_service_contract::assign_node_works_for_any_non_revoked_state | 🔲 |
| enrollment-interfaces.md § AssignNode | Ops can assign own vCluster | enrollment_service_contract::assign_node_allowed_for_ops_own_vcluster | 🔲 |
| enrollment-interfaces.md § AssignNode | Ops denied other vCluster | enrollment_service_contract::assign_node_denied_for_ops_other_vcluster | 🔲 |
| enrollment-interfaces.md § MoveNode | Move preserves cert serial | enrollment_service_contract::move_node_preserves_cert_serial | 🔲 |
| shared-kernel.md § NodeEnrollment | Serde round-trip | enrollment_data_contract::node_enrollment_round_trip | 🔲 |
| shared-kernel.md § HardwareIdentity | Optional TPM round-trip | enrollment_data_contract::hardware_identity_optional_tpm_round_trip | 🔲 |
| shared-kernel.md § SignedCert | No private key field | enrollment_data_contract::signed_cert_has_no_private_key_field | 🔲 |
| shared-kernel.md § EnrollmentState | All 4 variants exist | enrollment_data_contract::enrollment_state_has_all_variants | 🔲 |
| shared-kernel.md § HardwareIdentity | Requires MAC | enrollment_data_contract::hardware_identity_requires_at_least_one_mac | 🔲 |
| shared-kernel.md § HardwareIdentity | Requires BMC serial | enrollment_data_contract::hardware_identity_requires_bmc_serial | 🔲 |
| shared-kernel.md § NodeEnrollment | None vCluster valid | enrollment_data_contract::enrollment_with_none_vcluster_is_valid | 🔲 |
| invariants.md § E1 | Unenrolled node rejected | enrollment_invariant_contract::e1_unenrolled_node_cannot_get_certificate | 🔲 |
| invariants.md § E2 | Duplicate hardware rejected | enrollment_invariant_contract::e2_duplicate_hardware_identity_rejected | 🔲 |
| invariants.md § E4 | CSR returns only cert | enrollment_invariant_contract::e4_ca_key_manager_sign_csr_returns_only_cert | 🔲 |
| invariants.md § E4 | Raft state has no key material | enrollment_invariant_contract::e4_enrollment_record_in_raft_has_no_key_material | 🔲 |
| invariants.md § E7 | State machine transitions | enrollment_invariant_contract::e7_enrollment_state_machine_transitions | 🔲 |
| invariants.md § E8 | Cert CN has no vCluster | enrollment_invariant_contract::e8_cert_cn_contains_no_vcluster | 🔲 |
| invariants.md § E9 | Decommission revokes cert | enrollment_invariant_contract::e9_decommission_transitions_to_revoked_and_revokes_cert | 🔲 |
| invariants.md § E10 | RBAC enforcement | enrollment_invariant_contract::e10_rbac_enforcement_for_enrollment_operations | 🔲 |
| failure-modes.md § F18 | CA signing continues | enrollment_failure_contract::f18_ca_signing_continues_when_vault_unreachable | 🔲 |
| failure-modes.md § F18 | CA expiry detected | enrollment_failure_contract::f18_ca_key_approaching_expiry_detected | 🔲 |
| failure-modes.md § F19 | Active channel survives | enrollment_failure_contract::f19_active_channel_survives_renewal_failure | 🔲 |
| failure-modes.md § F19 | Degraded only on expiry | enrollment_failure_contract::f19_degraded_mode_only_on_actual_expiry | 🔲 |
| failure-modes.md § F20 | Rejection is retryable | enrollment_failure_contract::f20_enrollment_rejection_is_retryable | 🔲 |
| failure-modes.md § F20 | Not-enrolled vs revoked | enrollment_failure_contract::f20_not_enrolled_distinct_from_revoked | 🔲 |

## Contracts NOT covered (and why)

| Contract | Reason |
|---|---|
| E3 (single activation across domains) | Physical constraint — cannot be unit-tested. Covered by multi-domain BDD scenarios. |
| E6 (dual-channel rotation atomicity) | Requires live gRPC channel swap. Covered by integration/E2E tests, not contract tests. |
| Rate limiting (enrollment endpoint) | Infrastructure concern. Covered by BDD scenario "Enrollment endpoint is rate-limited". |
| Heartbeat timeout (Active → Inactive) | Requires timer-based state transition. Covered by BDD scenario "Active node detected as inactive on stream disconnect". |
| Batch enrollment partial failure | Composition of individual RegisterNode contracts. Covered by BDD scenario. |
