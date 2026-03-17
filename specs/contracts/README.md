# Contract Tests — pact System

Contract tests verify the integration surfaces between modules. They test BOUNDARIES, not internals.

## How to run

These are Rust-flavored test specifications. They become executable when types and interfaces are implemented.

```bash
cargo test -p pact-acceptance --test contracts
```

## Test files

| Category | File | What it tests | Count |
|---|---|---|---|
| Interface | `interface_contracts/enrollment_service_contract.rs` | EnrollmentService gRPC boundary (Enroll, RenewCert, RegisterNode, DecommissionNode, AssignNode, MoveNode) | 18 |
| Interface | `interface_contracts/journal_service_contract.rs` | ConfigService, PolicyService, BootConfigService, Raft state machine, Telemetry | 22 |
| Interface | `interface_contracts/policy_engine_contract.rs` | PolicyEngine, TokenValidator, RbacEngine, OpaClient, PolicyCache, FederationSync | 23 |
| Interface | `interface_contracts/hpc_auth_contract.rs` | AuthClient, TokenCache, DiscoveryCache, TokenSet Display, AuthError | 20 |
| Interface | `interface_contracts/agent_service_contract.rs` | ServiceManager, GpuBackend, StateObserver, DriftEvaluator, CommitWindowManager, ShellService | 31 |
| Data | `data_contracts/enrollment_data_contract.rs` | Enrollment type round-trips, validation constraints | 7 |
| Data | `data_contracts/shared_kernel_data_contract.rs` | Identity, ConfigEntry, StateDelta, DriftVector, VClusterPolicy, BootOverlay, ServiceDecl, CapabilityReport, AdminOperation, PendingApproval | 30 |
| Event | `event_contracts/event_schema_contract.rs` | ConfigEntry variant schemas, DriftEvent, AdminOperation, PendingApproval, MergeConflict, CapabilityReport, Loki envelope | 25 |
| Event | `event_contracts/event_producer_consumer_contract.rs` | Journal event flows, agent runtime drift pipeline, conflict events, enrollment events, malformed event handling | 23 |
| Invariant | `invariant_contracts/enrollment_invariant_contract.rs` | E1, E2, E4, E7, E8, E9, E10 enforcement | 8 |
| Invariant | `invariant_contracts/journal_invariant_contract.rs` | J1-J9 enforcement (sequences, immutability, auth, acyclicity, overlays, policy, Raft, reads, concurrency) | 22 |
| Invariant | `invariant_contracts/policy_invariant_contract.rs` | P1-P8 enforcement (auth, authz, scoping, two-person, timeout, admin bypass, degraded, AI restriction) | 14 |
| Invariant | `invariant_contracts/agent_invariant_contract.rs` | A1-A10, D1-D5 enforcement (commit windows, emergency, formula, rollback, consumers, ordering, cache, whitelist, blacklist, dimensions, magnitude, weights, observe-only) | 20 |
| Invariant | `invariant_contracts/conflict_invariant_contract.rs` | CR1-CR6, ND1-ND3 enforcement (merge conflicts, grace period, promote, notifications, cross-vCluster, TTL bounds, homogeneity) | 14 |
| Invariant | `invariant_contracts/shell_invariant_contract.rs` | S1-S6 enforcement (whitelist, admin bypass, rbash, audit, state-changing exec, no pre-classify) | 11 |
| Invariant | `invariant_contracts/auth_invariant_contract.rs` | Auth1-Auth8, PAuth1-PAuth5 enforcement (token validation, cache corruption, concurrent refresh, logout, permissions, isolation, redaction, cascade, strict mode, emergency human-only, discovery, break-glass, two-person) | 15 |
| Failure | `failure_contracts/enrollment_failure_contract.rs` | F18, F19, F20 degradation (CA key rotation, cert renewal failure, hardware mismatch) | 6 |
| Failure | `failure_contracts/journal_failure_contract.rs` | F1, F7, F8, F9 degradation (quorum loss, OPA crash, leader failover, stale overlay) | 10 |
| Failure | `failure_contracts/agent_failure_contract.rs` | F2, F3, F4, F5, F6 degradation (policy unreachable, partition, stale emergency, active consumers, crash recovery) | 13 |
| Failure | `failure_contracts/auth_failure_contract.rs` | F15, F16, F17 degradation (IdP unreachable, cache corrupted, stale discovery) | 7 |
| Failure | `failure_contracts/conflict_failure_contract.rs` | F13, F14 degradation (merge conflict on reconnect, promote conflicts) | 9 |

**Total: 348 contract tests across 21 files**

## Coverage Matrix

### Interface Contracts

| Contract Source | Contract Description | Test File | Test Name | Status |
|---|---|---|---|---|
| enrollment-interfaces\.md § Enroll | Reject unknown hardware identity | enrollment_service_contract | `enroll_rejects_unknown_hardware_identity` | :white_large_square: |
| enrollment-interfaces\.md § Enroll | Sign CSR for Registered node | enrollment_service_contract | `enroll_signs_csr_for_registered_node` | :white_large_square: |
| enrollment-interfaces\.md § Enroll | Reject Active node (once-Active) | enrollment_service_contract | `enroll_rejects_already_active_node` | :white_large_square: |
| enrollment-interfaces\.md § Enroll | Reject Revoked node | enrollment_service_contract | `enroll_rejects_revoked_node` | :white_large_square: |
| enrollment-interfaces\.md § Enroll | Inactive to Active succeeds | enrollment_service_contract | `enroll_succeeds_for_inactive_node` | :white_large_square: |
| enrollment-interfaces\.md § Enroll | Response includes vCluster | enrollment_service_contract | `enroll_response_includes_vcluster_assignment` | :white_large_square: |
| enrollment-interfaces\.md § Enroll | Response None for unassigned | enrollment_service_contract | `enroll_response_none_vcluster_for_unassigned` | :white_large_square: |
| enrollment-interfaces\.md § RenewCert | Sign new CSR on renewal | enrollment_service_contract | `renew_cert_signs_new_csr` | :white_large_square: |
| enrollment-interfaces\.md § RenewCert | Reject mismatched serial | enrollment_service_contract | `renew_cert_rejects_mismatched_serial` | :white_large_square: |
| enrollment-interfaces\.md § RegisterNode | Reject non-admin | enrollment_service_contract | `register_node_rejects_non_admin` | :white_large_square: |
| enrollment-interfaces\.md § RegisterNode | Reject duplicate hardware | enrollment_service_contract | `register_node_rejects_duplicate_hardware_identity` | :white_large_square: |
| enrollment-interfaces\.md § RegisterNode | Reject duplicate node_id | enrollment_service_contract | `register_node_rejects_duplicate_node_id` | :white_large_square: |
| enrollment-interfaces\.md § DecommissionNode | Warn on active sessions | enrollment_service_contract | `decommission_warns_on_active_sessions_without_force` | :white_large_square: |
| enrollment-interfaces\.md § DecommissionNode | Force revokes node | enrollment_service_contract | `decommission_with_force_revokes_node` | :white_large_square: |
| enrollment-interfaces\.md § AssignNode | Works for non-revoked states | enrollment_service_contract | `assign_node_works_for_any_non_revoked_state` | :white_large_square: |
| enrollment-interfaces\.md § AssignNode | Ops can assign own vCluster | enrollment_service_contract | `assign_node_allowed_for_ops_own_vcluster` | :white_large_square: |
| enrollment-interfaces\.md § AssignNode | Ops denied other vCluster | enrollment_service_contract | `assign_node_denied_for_ops_other_vcluster` | :white_large_square: |
| enrollment-interfaces\.md § MoveNode | Move preserves cert serial | enrollment_service_contract | `move_node_preserves_cert_serial` | :white_large_square: |
| journal-interfaces\.md § ConfigService | Reject empty principal | journal_service_contract | `append_entry_validates_non_empty_author` | :white_large_square: |
| journal-interfaces\.md § ConfigService | Reject empty role | journal_service_contract | `append_entry_validates_non_empty_role` | :white_large_square: |
| journal-interfaces\.md § ConfigService | Reject forward parent ref | journal_service_contract | `append_entry_validates_acyclic_parent` | :white_large_square: |
| journal-interfaces\.md § ConfigService | Append returns sequence | journal_service_contract | `append_entry_returns_sequence_number` | :white_large_square: |
| journal-interfaces\.md § ConfigService | Writes go through Raft | journal_service_contract | `append_entry_goes_through_raft` | :white_large_square: |
| journal-interfaces\.md § ConfigService | Reads from local state | journal_service_contract | `get_entry_reads_from_local_state` | :white_large_square: |
| journal-interfaces\.md § ConfigService | List entries ordered | journal_service_contract | `list_entries_returns_ordered_by_sequence` | :white_large_square: |
| journal-interfaces\.md § ConfigService | Overlay checksum validated | journal_service_contract | `get_overlay_validates_checksum` | :white_large_square: |
| journal-interfaces\.md § PolicyService | Platform admin always allowed | journal_service_contract | `evaluate_returns_allow_for_platform_admin` | :white_large_square: |
| journal-interfaces\.md § PolicyService | Unauthorized denied with reason | journal_service_contract | `evaluate_returns_deny_for_unauthorized` | :white_large_square: |
| journal-interfaces\.md § PolicyService | Two-person for regulated | journal_service_contract | `evaluate_returns_require_approval_for_regulated` | :white_large_square: |
| journal-interfaces\.md § PolicyService | Cached policy on OPA failure | journal_service_contract | `evaluate_falls_back_to_cached_on_opa_failure` | :white_large_square: |
| journal-interfaces\.md § PolicyService | Policy update via Raft | journal_service_contract | `update_policy_goes_through_raft` | :white_large_square: |
| journal-interfaces\.md § BootConfigService | Compressed chunks | journal_service_contract | `stream_boot_config_returns_compressed_chunks` | :white_large_square: |
| journal-interfaces\.md § BootConfigService | Complete message | journal_service_contract | `stream_boot_config_ends_with_complete_message` | :white_large_square: |
| journal-interfaces\.md § BootConfigService | Resume from sequence | journal_service_contract | `subscribe_config_updates_supports_from_sequence` | :white_large_square: |
| journal-interfaces\.md § BootConfigService | Push-based policy delivery | journal_service_contract | `subscribe_config_updates_delivers_policy_changes` | :white_large_square: |
| journal-interfaces\.md § Raft State Machine | Monotonic sequence | journal_service_contract | `apply_assigns_monotonic_sequence` | :white_large_square: |
| journal-interfaces\.md § Raft State Machine | Reject concurrent duplicate | journal_service_contract | `apply_rejects_concurrent_duplicate` | :white_large_square: |
| journal-interfaces\.md § Raft State Machine | Append-only | journal_service_contract | `state_machine_is_append_only` | :white_large_square: |
| journal-interfaces\.md § Telemetry | Health endpoint returns role | journal_service_contract | `health_endpoint_returns_role` | :white_large_square: |
| journal-interfaces\.md § Telemetry | Metrics on port 9091 | journal_service_contract | `metrics_endpoint_on_port_9091` | :white_large_square: |
| policy-interfaces\.md § PolicyEngine | Platform admin always allow | policy_engine_contract | `evaluate_platform_admin_always_allow` | :white_large_square: |
| policy-interfaces\.md § PolicyEngine | Viewer allow read, deny write | policy_engine_contract | `evaluate_viewer_allow_read_deny_write` | :white_large_square: |
| policy-interfaces\.md § PolicyEngine | Regulated two-person approval | policy_engine_contract | `evaluate_regulated_two_person_approval` | :white_large_square: |
| policy-interfaces\.md § PolicyEngine | Self-approval denied | policy_engine_contract | `evaluate_self_approval_denied` | :white_large_square: |
| policy-interfaces\.md § PolicyEngine | AI agent emergency denied | policy_engine_contract | `evaluate_ai_agent_emergency_denied` | :white_large_square: |
| policy-interfaces\.md § PolicyEngine | Get effective policy | policy_engine_contract | `get_effective_policy_returns_stored_policy` | :white_large_square: |
| policy-interfaces\.md § PolicyEngine | Federation overrides merged | policy_engine_contract | `get_effective_policy_merges_federation_overrides` | :white_large_square: |
| policy-interfaces\.md § TokenValidator | Valid token returns Identity | policy_engine_contract | `validate_returns_identity_for_valid_token` | :white_large_square: |
| policy-interfaces\.md § TokenValidator | Expired token rejected | policy_engine_contract | `validate_rejects_expired_token` | :white_large_square: |
| policy-interfaces\.md § TokenValidator | Wrong audience rejected | policy_engine_contract | `validate_rejects_wrong_audience` | :white_large_square: |
| policy-interfaces\.md § RbacEngine | Matching scope allowed | policy_engine_contract | `evaluate_allows_matching_scope` | :white_large_square: |
| policy-interfaces\.md § RbacEngine | Mismatched scope denied | policy_engine_contract | `evaluate_denies_mismatched_scope` | :white_large_square: |
| policy-interfaces\.md § RbacEngine | Complex rules defer to OPA | policy_engine_contract | `evaluate_defers_complex_rules_to_opa` | :white_large_square: |
| policy-interfaces\.md § RbacEngine | Platform admin early allow | policy_engine_contract | `evaluate_platform_admin_early_allow` | :white_large_square: |
| policy-interfaces\.md § OpaClient | Correct input format | policy_engine_contract | `evaluate_sends_correct_input_format` | :white_large_square: |
| policy-interfaces\.md § OpaClient | Allow/deny with reason | policy_engine_contract | `evaluate_returns_allow_deny_with_reason` | :white_large_square: |
| policy-interfaces\.md § OpaClient | Health detects unavailability | policy_engine_contract | `health_returns_false_when_sidecar_down` | :white_large_square: |
| policy-interfaces\.md § PolicyCache | Degraded whitelist honored | policy_engine_contract | `evaluate_degraded_honors_whitelist` | :white_large_square: |
| policy-interfaces\.md § PolicyCache | Degraded denies two-person | policy_engine_contract | `evaluate_degraded_denies_two_person` | :white_large_square: |
| policy-interfaces\.md § PolicyCache | Degraded denies complex OPA | policy_engine_contract | `evaluate_degraded_denies_complex_opa` | :white_large_square: |
| policy-interfaces\.md § PolicyCache | Degraded allows platform admin | policy_engine_contract | `evaluate_degraded_allows_platform_admin` | :white_large_square: |
| policy-interfaces\.md § FederationSync | Pulls Rego templates | policy_engine_contract | `sync_pulls_rego_templates` | :white_large_square: |
| policy-interfaces\.md § FederationSync | Cached on failure | policy_engine_contract | `sync_uses_cached_on_failure` | :white_large_square: |
| hpc-auth\.md § AuthClient::login | Token cache permissions 0600 | hpc_auth_contract | `login_stores_tokens_with_correct_permissions` | :white_large_square: |
| hpc-auth\.md § AuthClient::login | Cascading flow selection | hpc_auth_contract | `login_selects_flow_per_cascade` | :white_large_square: |
| hpc-auth\.md § AuthClient::logout | Cache cleared before IdP revocation | hpc_auth_contract | `logout_clears_cache_before_idp_revocation` | :white_large_square: |
| hpc-auth\.md § AuthClient::logout | Cache cleared even on IdP failure | hpc_auth_contract | `logout_clears_cache_even_on_idp_failure` | :white_large_square: |
| hpc-auth\.md § AuthClient::get_token | Cached token returned if valid | hpc_auth_contract | `get_token_returns_cached_if_valid` | :white_large_square: |
| hpc-auth\.md § AuthClient::get_token | Silent refresh if expired | hpc_auth_contract | `get_token_refreshes_silently_if_expired` | :white_large_square: |
| hpc-auth\.md § AuthClient::get_token | Error if both tokens expired | hpc_auth_contract | `get_token_returns_error_if_both_expired` | :white_large_square: |
| hpc-auth\.md § TokenCache::read | Strict mode rejects wrong perms | hpc_auth_contract | `read_validates_permissions_strict` | :white_large_square: |
| hpc-auth\.md § TokenCache::read | Lenient mode warns and fixes | hpc_auth_contract | `read_validates_permissions_lenient` | :white_large_square: |
| hpc-auth\.md § TokenCache::read | Corrupted JSON rejected | hpc_auth_contract | `read_rejects_corrupted_json` | :white_large_square: |
| hpc-auth\.md § TokenCache::write | File created with 0600 | hpc_auth_contract | `write_creates_file_with_0600` | :white_large_square: |
| hpc-auth\.md § TokenCache | Per-server isolation | hpc_auth_contract | `per_server_isolation` | :white_large_square: |
| hpc-auth\.md § TokenCache::list_servers | Lists all cached servers | hpc_auth_contract | `list_servers_returns_all_cached` | :white_large_square: |
| hpc-auth\.md § TokenCache::default_server | Default server round-trip | hpc_auth_contract | `default_server_roundtrip` | :white_large_square: |
| hpc-auth\.md § DiscoveryCache::get | Cached if fresh | hpc_auth_contract | `get_returns_cached_if_fresh` | :white_large_square: |
| hpc-auth\.md § DiscoveryCache::get | Fetches on stale/missing | hpc_auth_contract | `get_fetches_on_stale_or_missing` | :white_large_square: |
| hpc-auth\.md § DiscoveryCache::get | Returns stale on fetch failure | hpc_auth_contract | `get_returns_stale_on_fetch_failure` | :white_large_square: |
| hpc-auth\.md § DiscoveryCache::clear | Forces refetch | hpc_auth_contract | `clear_forces_refetch` | :white_large_square: |
| hpc-auth\.md § TokenSet Display | Refresh token redacted | hpc_auth_contract | `token_set_display_redacts_refresh_token` | :white_large_square: |
| hpc-auth\.md § AuthError | Error variants are distinct | hpc_auth_contract | `errors_are_distinct` | :white_large_square: |
| agent-interfaces\.md § ServiceManager | Start logs lifecycle | agent_service_contract | `start_logs_service_lifecycle_to_journal` | :white_large_square: |
| agent-interfaces\.md § ServiceManager | Stop uses reverse order | agent_service_contract | `stop_uses_reverse_dependency_order` | :white_large_square: |
| agent-interfaces\.md § ServiceManager | RestartPolicy::Always | agent_service_contract | `restart_respects_restart_policy_always` | :white_large_square: |
| agent-interfaces\.md § ServiceManager | RestartPolicy::Never | agent_service_contract | `restart_respects_restart_policy_never` | :white_large_square: |
| agent-interfaces\.md § ServiceManager | RestartPolicy::OnFailure | agent_service_contract | `restart_respects_restart_policy_on_failure` | :white_large_square: |
| agent-interfaces\.md § ServiceManager | Status returns pid/uptime | agent_service_contract | `status_returns_service_instance_with_pid` | :white_large_square: |
| agent-interfaces\.md § ServiceManager | Health check process type | agent_service_contract | `health_check_process_type` | :white_large_square: |
| agent-interfaces\.md § ServiceManager | Dependency ordering | agent_service_contract | `start_respects_dependency_ordering` | :white_large_square: |
| agent-interfaces\.md § GpuBackend | Detect returns capabilities | agent_service_contract | `detect_returns_gpu_capabilities` | :white_large_square: |
| agent-interfaces\.md § GpuBackend | Empty on non-GPU node | agent_service_contract | `detect_returns_empty_on_no_gpus` | :white_large_square: |
| agent-interfaces\.md § StateObserver | Events through channel | agent_service_contract | `observer_emits_drift_events_through_channel` | :white_large_square: |
| agent-interfaces\.md § StateObserver | Blacklisted paths filtered | agent_service_contract | `observer_filters_blacklisted_paths` | :white_large_square: |
| agent-interfaces\.md § StateObserver | Multiple observers compose | agent_service_contract | `multiple_observers_feed_same_evaluator` | :white_large_square: |
| agent-interfaces\.md § DriftEvaluator | Blacklisted returns None | agent_service_contract | `evaluate_returns_none_for_blacklisted` | :white_large_square: |
| agent-interfaces\.md § DriftEvaluator | Non-blacklisted returns vector | agent_service_contract | `evaluate_returns_drift_vector_for_valid_event` | :white_large_square: |
| agent-interfaces\.md § DriftEvaluator | Weighted Euclidean norm | agent_service_contract | `magnitude_uses_weighted_euclidean_norm` | :white_large_square: |
| agent-interfaces\.md § DriftEvaluator | Magnitude non-negative | agent_service_contract | `magnitude_non_negative` | :white_large_square: |
| agent-interfaces\.md § DriftEvaluator | Zero weight ignores dimension | agent_service_contract | `zero_weight_ignores_dimension` | :white_large_square: |
| agent-interfaces\.md § CommitWindowManager | Single window | agent_service_contract | `open_creates_single_window` | :white_large_square: |
| agent-interfaces\.md § CommitWindowManager | Extends existing window | agent_service_contract | `open_extends_existing_window_on_new_drift` | :white_large_square: |
| agent-interfaces\.md § CommitWindowManager | Commit closes window | agent_service_contract | `commit_closes_window_and_records_in_journal` | :white_large_square: |
| agent-interfaces\.md § CommitWindowManager | Rollback checks consumers | agent_service_contract | `rollback_checks_active_consumers` | :white_large_square: |
| agent-interfaces\.md § CommitWindowManager | Auto-rollback on expiry | agent_service_contract | `tick_auto_rollback_on_expiry` | :white_large_square: |
| agent-interfaces\.md § CommitWindowManager | Emergency suspends rollback | agent_service_contract | `tick_emergency_suspends_auto_rollback` | :white_large_square: |
| agent-interfaces\.md § CommitWindowManager | Formula always positive | agent_service_contract | `window_formula_always_positive` | :white_large_square: |
| agent-interfaces\.md § CommitWindowManager | Observe-only mode | agent_service_contract | `observe_only_mode_does_not_open_window` | :white_large_square: |
| agent-interfaces\.md § ShellService | Whitelist enforcement | agent_service_contract | `exec_rejects_non_whitelisted_command` | :white_large_square: |
| agent-interfaces\.md § ShellService | Platform admin bypass | agent_service_contract | `exec_allows_platform_admin_bypass` | :white_large_square: |
| agent-interfaces\.md § ShellService | Restricted bash | agent_service_contract | `shell_uses_restricted_bash` | :white_large_square: |
| agent-interfaces\.md § ShellService | Exec logged to journal | agent_service_contract | `exec_logs_to_journal` | :white_large_square: |
| agent-interfaces\.md § ShellService | State-changing opens window | agent_service_contract | `state_changing_exec_opens_commit_window` | :white_large_square: |

### Data Contracts

| Contract Source | Contract Description | Test File | Test Name | Status |
|---|---|---|---|---|
| shared-kernel\.md § NodeEnrollment | Serde round-trip | enrollment_data_contract | `node_enrollment_round_trip` | :white_large_square: |
| shared-kernel\.md § HardwareIdentity | Optional TPM round-trip | enrollment_data_contract | `hardware_identity_optional_tpm_round_trip` | :white_large_square: |
| shared-kernel\.md § SignedCert | No private key field | enrollment_data_contract | `signed_cert_has_no_private_key_field` | :white_large_square: |
| shared-kernel\.md § EnrollmentState | All 4 variants exist | enrollment_data_contract | `enrollment_state_has_all_variants` | :white_large_square: |
| shared-kernel\.md § HardwareIdentity | Requires MAC | enrollment_data_contract | `hardware_identity_requires_at_least_one_mac` | :white_large_square: |
| shared-kernel\.md § HardwareIdentity | Requires BMC serial | enrollment_data_contract | `hardware_identity_requires_bmc_serial` | :white_large_square: |
| shared-kernel\.md § NodeEnrollment | None vCluster valid | enrollment_data_contract | `enrollment_with_none_vcluster_is_valid` | :white_large_square: |
| shared-kernel\.md § Identity | Serde round-trip | shared_kernel_data_contract | `identity_round_trip` | :white_large_square: |
| shared-kernel\.md § Identity | Rejects empty principal | shared_kernel_data_contract | `identity_rejects_empty_principal` | :white_large_square: |
| shared-kernel\.md § Identity | Rejects empty role | shared_kernel_data_contract | `identity_rejects_empty_role` | :white_large_square: |
| shared-kernel\.md § PrincipalType | All 3 variants | shared_kernel_data_contract | `principal_type_all_variants` | :white_large_square: |
| shared-kernel\.md § RoleBinding | Serde round-trip | shared_kernel_data_contract | `role_binding_round_trip` | :white_large_square: |
| shared-kernel\.md § ConfigState | All 5 variants | shared_kernel_data_contract | `config_state_all_variants` | :white_large_square: |
| shared-kernel\.md § EntryType | All 21 variants | shared_kernel_data_contract | `entry_type_all_variants` | :white_large_square: |
| shared-kernel\.md § Scope | All 3 variants | shared_kernel_data_contract | `scope_all_variants` | :white_large_square: |
| shared-kernel\.md § ConfigEntry | Full round-trip | shared_kernel_data_contract | `config_entry_round_trip` | :white_large_square: |
| shared-kernel\.md § ConfigEntry | Optional fields None | shared_kernel_data_contract | `config_entry_optional_fields_none` | :white_large_square: |
| shared-kernel\.md § StateDelta | Round-trip with all actions | shared_kernel_data_contract | `state_delta_round_trip` | :white_large_square: |
| shared-kernel\.md § DriftVector | Round-trip all 7 dimensions | shared_kernel_data_contract | `drift_vector_round_trip` | :white_large_square: |
| shared-kernel\.md § DriftVector | Default all zero | shared_kernel_data_contract | `drift_vector_default_all_zero` | :white_large_square: |
| shared-kernel\.md § DriftWeights | Default values | shared_kernel_data_contract | `drift_weights_default_values` | :white_large_square: |
| shared-kernel\.md § VClusterPolicy | Full 17-field round-trip | shared_kernel_data_contract | `vcluster_policy_round_trip` | :white_large_square: |
| shared-kernel\.md § VClusterPolicy | Default is observe-only | shared_kernel_data_contract | `vcluster_policy_default_is_observe_only` | :white_large_square: |
| shared-kernel\.md § BootOverlay | Serde round-trip | shared_kernel_data_contract | `boot_overlay_round_trip` | :white_large_square: |
| shared-kernel\.md § BootOverlay | Checksum must match data | shared_kernel_data_contract | `boot_overlay_checksum_must_match_data` | :white_large_square: |
| shared-kernel\.md § ServiceState | All 6 variants | shared_kernel_data_contract | `service_state_all_variants` | :white_large_square: |
| shared-kernel\.md § ServiceDecl | Full round-trip | shared_kernel_data_contract | `service_decl_round_trip` | :white_large_square: |
| shared-kernel\.md § RestartPolicy | All 3 variants | shared_kernel_data_contract | `restart_policy_all_variants` | :white_large_square: |
| shared-kernel\.md § HealthCheckType | All 3 variants | shared_kernel_data_contract | `health_check_type_all_variants` | :white_large_square: |
| shared-kernel\.md § CapabilityReport | Full round-trip | shared_kernel_data_contract | `capability_report_round_trip` | :white_large_square: |
| shared-kernel\.md § GpuCapability | Serde round-trip | shared_kernel_data_contract | `gpu_capability_round_trip` | :white_large_square: |
| shared-kernel\.md § GpuHealth | All 3 variants | shared_kernel_data_contract | `gpu_health_all_variants` | :white_large_square: |
| shared-kernel\.md § SupervisorStatus | Serde round-trip | shared_kernel_data_contract | `supervisor_status_round_trip` | :white_large_square: |
| shared-kernel\.md § AdminOperation | Full round-trip | shared_kernel_data_contract | `admin_operation_round_trip` | :white_large_square: |
| shared-kernel\.md § AdminOperationType | All 14 variants | shared_kernel_data_contract | `admin_operation_type_all_variants` | :white_large_square: |
| shared-kernel\.md § PendingApproval | Full round-trip | shared_kernel_data_contract | `pending_approval_round_trip` | :white_large_square: |
| shared-kernel\.md § ApprovalStatus | All 4 variants | shared_kernel_data_contract | `approval_status_all_variants` | :white_large_square: |

### Event Contracts

| Contract Source | Contract Description | Test File | Test Name | Status |
|---|---|---|---|---|
| event-schemas\.md § ConfigEntry Commit | Delta yes, TTL no | event_schema_contract | `commit_entry_has_delta_no_ttl` | :white_large_square: |
| event-schemas\.md § ConfigEntry Rollback | Reverse delta | event_schema_contract | `rollback_entry_has_reverse_delta` | :white_large_square: |
| event-schemas\.md § ConfigEntry EmergencyStart | TTL present, no delta | event_schema_contract | `emergency_start_has_ttl_no_delta` | :white_large_square: |
| event-schemas\.md § ConfigEntry EmergencyEnd | Links to start | event_schema_contract | `emergency_end_links_to_start` | :white_large_square: |
| event-schemas\.md § ConfigEntry ExecLog | Command in metadata | event_schema_contract | `exec_log_has_command_in_metadata` | :white_large_square: |
| event-schemas\.md § ConfigEntry ShellSession | Action in metadata | event_schema_contract | `shell_session_has_action_in_metadata` | :white_large_square: |
| event-schemas\.md § ConfigEntry ServiceLifecycle | Service in metadata | event_schema_contract | `service_lifecycle_has_service_in_metadata` | :white_large_square: |
| event-schemas\.md § ConfigEntry PendingApproval | Timeout TTL | event_schema_contract | `pending_approval_has_timeout_ttl` | :white_large_square: |
| event-schemas\.md § ConfigEntry DriftDetected | Informational only | event_schema_contract | `drift_detected_is_informational` | :white_large_square: |
| event-schemas\.md § DriftEvent | Serde round-trip | event_schema_contract | `drift_event_round_trip` | :white_large_square: |
| event-schemas\.md § DriftSource | All 4 variants | event_schema_contract | `drift_source_all_variants` | :white_large_square: |
| event-schemas\.md § DriftDimension | Exactly 7 variants | event_schema_contract | `drift_dimension_exactly_seven` | :white_large_square: |
| event-schemas\.md § DriftVector | Magnitude formula | event_schema_contract | `drift_vector_magnitude_formula` | :white_large_square: |
| event-schemas\.md § AdminOperation | All types round-trip | event_schema_contract | `admin_operation_all_types_round_trip` | :white_large_square: |
| event-schemas\.md § AdminOperation | Requires identity | event_schema_contract | `admin_operation_requires_identity` | :white_large_square: |
| event-schemas\.md § PendingApproval | Approver distinct | event_schema_contract | `pending_approval_approver_distinct_from_requester` | :white_large_square: |
| event-schemas\.md § PendingApproval | Expires in future | event_schema_contract | `pending_approval_expires_at_in_future` | :white_large_square: |
| event-schemas\.md § ApprovalStatus | Valid transitions | event_schema_contract | `approval_status_transitions` | :white_large_square: |
| event-schemas\.md § MergeConflict | Serde round-trip | event_schema_contract | `merge_conflict_round_trip` | :white_large_square: |
| event-schemas\.md § ConflictEntry | Both values present | event_schema_contract | `conflict_entry_has_both_values` | :white_large_square: |
| event-schemas\.md § ConflictResolution | All 3 variants | event_schema_contract | `conflict_resolution_all_variants` | :white_large_square: |
| event-schemas\.md § PromoteConflict | Serde round-trip | event_schema_contract | `promote_conflict_round_trip` | :white_large_square: |
| event-schemas\.md § CapabilityReport | Supervisor status | event_schema_contract | `capability_report_includes_supervisor_status` | :white_large_square: |
| event-schemas\.md § CapabilityReport | Optional fields | event_schema_contract | `capability_report_optional_fields` | :white_large_square: |
| event-schemas\.md § Loki Event Schema | Required fields | event_schema_contract | `loki_event_has_required_fields` | :white_large_square: |
| event-catalog\.md § Journal Commit | Consumed by journal and CLI | event_producer_consumer_contract | `commit_entry_consumed_by_journal_and_cli` | :white_large_square: |
| event-catalog\.md § Journal Rollback | Links to original | event_producer_consumer_contract | `rollback_entry_links_to_original` | :white_large_square: |
| event-catalog\.md § Journal DriftDetected | Consumed by status | event_producer_consumer_contract | `drift_detected_consumed_by_status` | :white_large_square: |
| event-catalog\.md § Journal CapabilityChange | Consumed by scheduler | event_producer_consumer_contract | `capability_change_consumed_by_scheduler` | :white_large_square: |
| event-catalog\.md § Journal PolicyUpdate | Consumed by agents | event_producer_consumer_contract | `policy_update_consumed_by_agents` | :white_large_square: |
| event-catalog\.md § Journal EmergencyStart | Triggers Loki alert | event_producer_consumer_contract | `emergency_start_triggers_loki_alert` | :white_large_square: |
| event-catalog\.md § Admin Exec | Consumed by audit | event_producer_consumer_contract | `exec_log_consumed_by_audit` | :white_large_square: |
| event-catalog\.md § Journal ServiceLifecycle | Consumed by status | event_producer_consumer_contract | `service_lifecycle_consumed_by_status` | :white_large_square: |
| event-catalog\.md § Journal PendingApproval | Consumed by approve cmd | event_producer_consumer_contract | `pending_approval_consumed_by_approve_command` | :white_large_square: |
| event-catalog\.md § Agent DriftEvent | Flows through mpsc | event_producer_consumer_contract | `drift_event_flows_through_mpsc_channel` | :white_large_square: |
| event-catalog\.md § Agent DriftEvent | Blacklisted filtered | event_producer_consumer_contract | `blacklisted_drift_event_filtered` | :white_large_square: |
| event-catalog\.md § Agent DriftEvent | Aggregates into vector | event_producer_consumer_contract | `drift_event_aggregates_into_drift_vector` | :white_large_square: |
| event-catalog\.md § Agent DriftEvent | Observe-only logged not enforced | event_producer_consumer_contract | `observe_only_drift_event_logged_not_enforced` | :white_large_square: |
| event-catalog\.md § MergeConflictDetected | Produced on reconnect | event_producer_consumer_contract | `merge_conflict_produced_on_reconnect` | :white_large_square: |
| event-catalog\.md § MergeConflictDetected | Consumed by CLI notification | event_producer_consumer_contract | `merge_conflict_consumed_by_cli_notification` | :white_large_square: |
| event-catalog\.md § GracePeriodOverwrite | Produces event | event_producer_consumer_contract | `grace_period_overwrite_produces_event` | :white_large_square: |
| event-catalog\.md § PromoteConflictDetected | Blocks CLI | event_producer_consumer_contract | `promote_conflict_blocks_cli` | :white_large_square: |
| event-catalog\.md § Invariants J3/J7 | Malformed entry rejected | event_producer_consumer_contract | `malformed_config_entry_rejected` | :white_large_square: |
| event-catalog\.md § Invariants J3 | Missing author rejected | event_producer_consumer_contract | `config_entry_with_missing_author_rejected` | :white_large_square: |
| event-catalog\.md § Agent DriftEvent | Unknown dimension rejected | event_producer_consumer_contract | `drift_event_with_unknown_dimension_rejected` | :white_large_square: |
| event-catalog\.md § Journal NodeEnrolled | Produced on register | event_producer_consumer_contract | `node_enrolled_event_produced_on_register` | :white_large_square: |
| event-catalog\.md § Journal CertSigned | Produced on enroll | event_producer_consumer_contract | `cert_signed_event_produced_on_enroll` | :white_large_square: |
| event-catalog\.md § Journal CertRevoked | Produced on decommission | event_producer_consumer_contract | `cert_revoked_event_produced_on_decommission` | :white_large_square: |

### Invariant Contracts

| Contract Source | Contract Description | Test File | Test Name | Status |
|---|---|---|---|---|
| invariants\.md § E1 | Unenrolled node rejected | enrollment_invariant_contract | `e1_unenrolled_node_cannot_get_certificate` | :white_large_square: |
| invariants\.md § E2 | Duplicate hardware rejected | enrollment_invariant_contract | `e2_duplicate_hardware_identity_rejected` | :white_large_square: |
| invariants\.md § E4 | CSR returns only cert | enrollment_invariant_contract | `e4_ca_key_manager_sign_csr_returns_only_cert` | :white_large_square: |
| invariants\.md § E4 | Raft state has no key material | enrollment_invariant_contract | `e4_enrollment_record_in_raft_has_no_key_material` | :white_large_square: |
| invariants\.md § E7 | State machine transitions | enrollment_invariant_contract | `e7_enrollment_state_machine_transitions` | :white_large_square: |
| invariants\.md § E8 | Cert CN has no vCluster | enrollment_invariant_contract | `e8_cert_cn_contains_no_vcluster` | :white_large_square: |
| invariants\.md § E9 | Decommission revokes cert | enrollment_invariant_contract | `e9_decommission_transitions_to_revoked_and_revokes_cert` | :white_large_square: |
| invariants\.md § E10 | RBAC enforcement | enrollment_invariant_contract | `e10_rbac_enforcement_for_enrollment_operations` | :white_large_square: |
| invariants\.md § J1 | Sequences strictly increasing | journal_invariant_contract | `j1_sequences_strictly_increasing` | :white_large_square: |
| invariants\.md § J1 | No gaps in sequence | journal_invariant_contract | `j1_no_gaps_in_sequence` | :white_large_square: |
| invariants\.md § J2 | No update method exists | journal_invariant_contract | `j2_no_update_method_exists` | :white_large_square: |
| invariants\.md § J2 | Entries unchanged after commit | journal_invariant_contract | `j2_entries_unchanged_after_commit` | :white_large_square: |
| invariants\.md § J3 | Empty principal rejected | journal_invariant_contract | `j3_empty_principal_rejected` | :white_large_square: |
| invariants\.md § J3 | Empty role rejected | journal_invariant_contract | `j3_empty_role_rejected` | :white_large_square: |
| invariants\.md § J3 | Valid author accepted | journal_invariant_contract | `j3_valid_author_accepted` | :white_large_square: |
| invariants\.md § J4 | Parent < sequence accepted | journal_invariant_contract | `j4_parent_less_than_sequence_accepted` | :white_large_square: |
| invariants\.md § J4 | Parent == sequence rejected | journal_invariant_contract | `j4_parent_equal_to_sequence_rejected` | :white_large_square: |
| invariants\.md § J4 | Parent > sequence rejected | journal_invariant_contract | `j4_parent_greater_than_sequence_rejected` | :white_large_square: |
| invariants\.md § J4 | None parent accepted | journal_invariant_contract | `j4_none_parent_always_accepted` | :white_large_square: |
| invariants\.md § J5 | Matching checksum accepted | journal_invariant_contract | `j5_matching_checksum_accepted` | :white_large_square: |
| invariants\.md § J5 | Mismatched checksum rejected | journal_invariant_contract | `j5_mismatched_checksum_rejected` | :white_large_square: |
| invariants\.md § J6 | Set policy replaces existing | journal_invariant_contract | `j6_set_policy_replaces_existing` | :white_large_square: |
| invariants\.md § J6 | Different vClusters coexist | journal_invariant_contract | `j6_different_vclusters_coexist` | :white_large_square: |
| invariants\.md § J7 | AppendEntry through Raft | journal_invariant_contract | `j7_append_entry_goes_through_raft` | :white_large_square: |
| invariants\.md § J7 | SetPolicy through Raft | journal_invariant_contract | `j7_set_policy_goes_through_raft` | :white_large_square: |
| invariants\.md § J7 | No direct state mutation | journal_invariant_contract | `j7_no_direct_state_mutation` | :white_large_square: |
| invariants\.md § J8 | get_entry reads local | journal_invariant_contract | `j8_get_entry_reads_local` | :white_large_square: |
| invariants\.md § J8 | list_entries reads local | journal_invariant_contract | `j8_list_entries_reads_local` | :white_large_square: |
| invariants\.md § J8 | get_overlay reads local | journal_invariant_contract | `j8_get_overlay_reads_local` | :white_large_square: |
| invariants\.md § J9 | Concurrent appends unique | journal_invariant_contract | `j9_concurrent_appends_get_unique_sequences` | :white_large_square: |
| invariants\.md § P1 | Unauthenticated rejected | policy_invariant_contract | `p1_unauthenticated_request_rejected` | :white_large_square: |
| invariants\.md § P1 | Invalid token rejected | policy_invariant_contract | `p1_invalid_token_rejected` | :white_large_square: |
| invariants\.md § P2 | Unauthorized denied | policy_invariant_contract | `p2_unauthorized_operation_denied` | :white_large_square: |
| invariants\.md § P3 | Role scoped to vCluster | policy_invariant_contract | `p3_role_scoped_to_vcluster` | :white_large_square: |
| invariants\.md § P3 | Role allows own vCluster | policy_invariant_contract | `p3_role_scoping_allows_own_vcluster` | :white_large_square: |
| invariants\.md § P4 | Two-person approval required | policy_invariant_contract | `p4_two_person_approval_required` | :white_large_square: |
| invariants\.md § P4 | Self-approval denied | policy_invariant_contract | `p4_self_approval_denied` | :white_large_square: |
| invariants\.md § P5 | Expired approval rejected | policy_invariant_contract | `p5_expired_approval_rejected` | :white_large_square: |
| invariants\.md § P6 | Platform admin always allowed | policy_invariant_contract | `p6_platform_admin_always_allowed` | :white_large_square: |
| invariants\.md § P6 | Platform admin still logged | policy_invariant_contract | `p6_platform_admin_still_logged` | :white_large_square: |
| invariants\.md § P7 | Degraded whitelist honored | policy_invariant_contract | `p7_degraded_whitelist_honored` | :white_large_square: |
| invariants\.md § P7 | Degraded two-person denied | policy_invariant_contract | `p7_degraded_two_person_denied` | :white_large_square: |
| invariants\.md § P7 | Degraded OPA denied | policy_invariant_contract | `p7_degraded_opa_denied` | :white_large_square: |
| invariants\.md § P8 | AI agent emergency denied | policy_invariant_contract | `p8_ai_agent_emergency_denied` | :white_large_square: |
| invariants\.md § A1 | At most one commit window | agent_invariant_contract | `a1_at_most_one_commit_window` | :white_large_square: |
| invariants\.md § A1 | New drift extends window | agent_invariant_contract | `a1_new_drift_extends_existing_window` | :white_large_square: |
| invariants\.md § A2 | At most one emergency | agent_invariant_contract | `a2_at_most_one_emergency_session` | :white_large_square: |
| invariants\.md § A3 | Formula always positive | agent_invariant_contract | `a3_commit_window_formula_always_positive` | :white_large_square: |
| invariants\.md § A3 | Formula boundary values | agent_invariant_contract | `a3_commit_window_formula_boundary_values` | :white_large_square: |
| invariants\.md § A4 | Auto-rollback on expiry | agent_invariant_contract | `a4_auto_rollback_on_window_expiry` | :white_large_square: |
| invariants\.md § A4 | Emergency suspends rollback | agent_invariant_contract | `a4_emergency_suspends_auto_rollback` | :white_large_square: |
| invariants\.md § A5 | Rollback blocked by consumers | agent_invariant_contract | `a5_rollback_blocked_by_active_consumers` | :white_large_square: |
| invariants\.md § A6 | Services start in order | agent_invariant_contract | `a6_services_start_in_order` | :white_large_square: |
| invariants\.md § A6 | Services stop in reverse | agent_invariant_contract | `a6_services_stop_in_reverse_order` | :white_large_square: |
| invariants\.md § A9 | Cached config during partition | agent_invariant_contract | `a9_cached_config_during_partition` | :white_large_square: |
| invariants\.md § A10 | Emergency no whitelist expand | agent_invariant_contract | `a10_emergency_does_not_expand_whitelist` | :white_large_square: |
| invariants\.md § D1 | Default blacklist paths | agent_invariant_contract | `d1_blacklist_exclusion_default_paths` | :white_large_square: |
| invariants\.md § D1 | Non-blacklisted produces drift | agent_invariant_contract | `d1_non_blacklisted_path_produces_drift` | :white_large_square: |
| invariants\.md § D2 | Exactly 7 dimensions | agent_invariant_contract | `d2_exactly_seven_dimensions` | :white_large_square: |
| invariants\.md § D3 | Magnitude non-negative | agent_invariant_contract | `d3_magnitude_non_negative` | :white_large_square: |
| invariants\.md § D3 | All-zero is zero | agent_invariant_contract | `d3_all_zero_magnitude_is_zero` | :white_large_square: |
| invariants\.md § D4 | Kernel/GPU weighted double | agent_invariant_contract | `d4_kernel_and_gpu_weighted_double` | :white_large_square: |
| invariants\.md § D4 | Zero weight ignores | agent_invariant_contract | `d4_zero_weight_ignores_dimension` | :white_large_square: |
| invariants\.md § D5 | Observe-only logs only | agent_invariant_contract | `d5_observe_only_logs_without_enforcement` | :white_large_square: |
| invariants\.md § CR1 | Local changes fed back first | conflict_invariant_contract | `cr1_local_changes_fed_back_before_sync` | :white_large_square: |
| invariants\.md § CR2 | Merge conflict pauses convergence | conflict_invariant_contract | `cr2_merge_conflict_pauses_convergence` | :white_large_square: |
| invariants\.md § CR2 | Non-conflicting keys sync | conflict_invariant_contract | `cr2_non_conflicting_keys_sync_normally` | :white_large_square: |
| invariants\.md § CR3 | Grace period journal-wins | conflict_invariant_contract | `cr3_grace_period_fallback_to_journal_wins` | :white_large_square: |
| invariants\.md § CR3 | Overwritten changes logged | conflict_invariant_contract | `cr3_overwritten_changes_logged` | :white_large_square: |
| invariants\.md § CR4 | Promote blocked on conflicts | conflict_invariant_contract | `cr4_promote_blocked_on_conflicts` | :white_large_square: |
| invariants\.md § CR4 | Promote no conflicts proceeds | conflict_invariant_contract | `cr4_promote_no_conflicts_proceeds` | :white_large_square: |
| invariants\.md § CR5 | Admin notified on overwrite | conflict_invariant_contract | `cr5_admin_notified_on_overwrite` | :white_large_square: |
| invariants\.md § CR6 | No cross-vCluster atomicity | conflict_invariant_contract | `cr6_no_cross_vcluster_atomicity` | :white_large_square: |
| invariants\.md § ND1 | TTL minimum 15 minutes | conflict_invariant_contract | `nd1_ttl_minimum_15_minutes` | :white_large_square: |
| invariants\.md § ND1 | TTL exactly 900 accepted | conflict_invariant_contract | `nd1_ttl_exactly_15_minutes_accepted` | :white_large_square: |
| invariants\.md § ND2 | TTL maximum 10 days | conflict_invariant_contract | `nd2_ttl_maximum_10_days` | :white_large_square: |
| invariants\.md § ND2 | TTL exactly 864000 accepted | conflict_invariant_contract | `nd2_ttl_exactly_10_days_accepted` | :white_large_square: |
| invariants\.md § ND3 | Divergent nodes warned | conflict_invariant_contract | `nd3_divergent_nodes_warned` | :white_large_square: |
| invariants\.md § S1 | Non-whitelisted rejected | shell_invariant_contract | `s1_non_whitelisted_command_rejected` | :white_large_square: |
| invariants\.md § S1 | Whitelisted allowed | shell_invariant_contract | `s1_whitelisted_command_allowed` | :white_large_square: |
| invariants\.md § S2 | Platform admin bypasses | shell_invariant_contract | `s2_platform_admin_bypasses_whitelist` | :white_large_square: |
| invariants\.md § S2 | Bypass still logged | shell_invariant_contract | `s2_platform_admin_bypass_still_logged` | :white_large_square: |
| invariants\.md § S3 | Shell uses rbash | shell_invariant_contract | `s3_shell_uses_rbash` | :white_large_square: |
| invariants\.md § S3 | Cannot change PATH | shell_invariant_contract | `s3_rbash_cannot_change_path` | :white_large_square: |
| invariants\.md § S3 | Cannot redirect output | shell_invariant_contract | `s3_rbash_cannot_redirect` | :white_large_square: |
| invariants\.md § S4 | Exec logged to journal | shell_invariant_contract | `s4_exec_logged_to_journal` | :white_large_square: |
| invariants\.md § S4 | PROMPT_COMMAND logs commands | shell_invariant_contract | `s4_shell_command_logged_via_prompt_command` | :white_large_square: |
| invariants\.md § S5 | State-changing opens window | shell_invariant_contract | `s5_state_changing_exec_opens_commit_window` | :white_large_square: |
| invariants\.md § S6 | No pre-classification | shell_invariant_contract | `s6_shell_does_not_pre_classify` | :white_large_square: |
| invariants\.md § Auth1 | No unauthenticated commands | auth_invariant_contract | `auth1_no_unauthenticated_commands` | :white_large_square: |
| invariants\.md § Auth1 | Exempt commands | auth_invariant_contract | `auth1_exempt_commands` | :white_large_square: |
| invariants\.md § Auth2 | Corrupted cache rejected | auth_invariant_contract | `auth2_corrupted_cache_rejected` | :white_large_square: |
| invariants\.md § Auth3 | Concurrent refresh safe | auth_invariant_contract | `auth3_concurrent_refresh_safe` | :white_large_square: |
| invariants\.md § Auth4 | Logout deletes before revoke | auth_invariant_contract | `auth4_logout_deletes_before_revoke` | :white_large_square: |
| invariants\.md § Auth5 | Strict mode rejects wrong perms | auth_invariant_contract | `auth5_strict_mode_rejects_wrong_perms` | :white_large_square: |
| invariants\.md § Auth5 | Lenient mode fixes perms | auth_invariant_contract | `auth5_lenient_mode_fixes_perms` | :white_large_square: |
| invariants\.md § Auth6 | Per-server isolation | auth_invariant_contract | `auth6_per_server_isolation` | :white_large_square: |
| invariants\.md § Auth7 | Refresh token never logged | auth_invariant_contract | `auth7_refresh_token_never_logged` | :white_large_square: |
| invariants\.md § Auth8 | Cascade fallback | auth_invariant_contract | `auth8_cascade_fallback` | :white_large_square: |
| invariants\.md § PAuth1 | Pact uses strict mode | auth_invariant_contract | `pauth1_pact_uses_strict_mode` | :white_large_square: |
| invariants\.md § PAuth2 | Emergency requires human | auth_invariant_contract | `pauth2_emergency_requires_human` | :white_large_square: |
| invariants\.md § PAuth3 | Discovery endpoint public | auth_invariant_contract | `pauth3_discovery_endpoint_unauthenticated` | :white_large_square: |
| invariants\.md § PAuth4 | Break-glass is BMC | auth_invariant_contract | `pauth4_break_glass_is_bmc` | :white_large_square: |
| invariants\.md § PAuth5 | Two-person distinct identities | auth_invariant_contract | `pauth5_two_person_distinct_identities` | :white_large_square: |

### Failure Contracts

| Contract Source | Contract Description | Test File | Test Name | Status |
|---|---|---|---|---|
| failure-modes\.md § F18 | CA signing continues during key rotation | enrollment_failure_contract | `f18_ca_signing_continues_during_key_rotation` | :white_large_square: |
| failure-modes\.md § F18 | CA expiry detected | enrollment_failure_contract | `f18_ca_key_approaching_expiry_detected` | :white_large_square: |
| failure-modes\.md § F19 | Active channel survives renewal failure | enrollment_failure_contract | `f19_active_channel_survives_renewal_failure` | :white_large_square: |
| failure-modes\.md § F19 | Degraded only on actual expiry | enrollment_failure_contract | `f19_degraded_mode_only_on_actual_expiry` | :white_large_square: |
| failure-modes\.md § F20 | Enrollment rejection retryable | enrollment_failure_contract | `f20_enrollment_rejection_is_retryable` | :white_large_square: |
| failure-modes\.md § F20 | Not-enrolled vs revoked | enrollment_failure_contract | `f20_not_enrolled_distinct_from_revoked` | :white_large_square: |
| failure-modes\.md § F1 | Reads continue on quorum loss | journal_failure_contract | `f1_reads_continue_on_quorum_loss` | :white_large_square: |
| failure-modes\.md § F1 | Writes blocked on quorum loss | journal_failure_contract | `f1_writes_blocked_on_quorum_loss` | :white_large_square: |
| failure-modes\.md § F1 | Boot streaming continues | journal_failure_contract | `f1_boot_streaming_continues` | :white_large_square: |
| failure-modes\.md § F7 | Policy falls back to cached | journal_failure_contract | `f7_policy_falls_back_to_cached` | :white_large_square: |
| failure-modes\.md § F7 | Basic RBAC still works | journal_failure_contract | `f7_basic_rbac_still_works` | :white_large_square: |
| failure-modes\.md § F7 | Complex Rego denied | journal_failure_contract | `f7_complex_rego_denied` | :white_large_square: |
| failure-modes\.md § F8 | Reads during election | journal_failure_contract | `f8_reads_continue_during_election` | :white_large_square: |
| failure-modes\.md § F8 | Writes resume on new leader | journal_failure_contract | `f8_writes_resume_on_new_leader` | :white_large_square: |
| failure-modes\.md § F9 | Stale overlay detected | journal_failure_contract | `f9_stale_overlay_detected` | :white_large_square: |
| failure-modes\.md § F9 | On-demand rebuild triggered | journal_failure_contract | `f9_on_demand_rebuild_triggered` | :white_large_square: |
| failure-modes\.md § F2 | Cached whitelist honored | agent_failure_contract | `f2_cached_whitelist_honored` | :white_large_square: |
| failure-modes\.md § F2 | Two-person denied fail-closed | agent_failure_contract | `f2_two_person_denied_fail_closed` | :white_large_square: |
| failure-modes\.md § F2 | Platform admin cached | agent_failure_contract | `f2_platform_admin_authorized_cached` | :white_large_square: |
| failure-modes\.md § F3 | Agent continues cached config | agent_failure_contract | `f3_agent_continues_with_cached_config` | :white_large_square: |
| failure-modes\.md § F3 | Drift detection local | agent_failure_contract | `f3_drift_detection_continues_locally` | :white_large_square: |
| failure-modes\.md § F3 | Operations logged locally | agent_failure_contract | `f3_operations_logged_locally` | :white_large_square: |
| failure-modes\.md § F3 | Config subscription resumes | agent_failure_contract | `f3_config_subscription_resumes_from_sequence` | :white_large_square: |
| failure-modes\.md § F4 | Stale emergency detected | agent_failure_contract | `f4_stale_emergency_detected` | :white_large_square: |
| failure-modes\.md § F4 | Force-end requires admin | agent_failure_contract | `f4_force_end_requires_admin` | :white_large_square: |
| failure-modes\.md § F5 | Rollback fails with consumers | agent_failure_contract | `f5_rollback_fails_if_consumers_active` | :white_large_square: |
| failure-modes\.md § F5 | Failed rollback logged | agent_failure_contract | `f5_failed_rollback_logged` | :white_large_square: |
| failure-modes\.md § F6 | Agent re-authenticates | agent_failure_contract | `f6_agent_re_authenticates_on_restart` | :white_large_square: |
| failure-modes\.md § F6 | Cached config applied | agent_failure_contract | `f6_cached_config_applied_on_restart` | :white_large_square: |
| failure-modes\.md § F15 | Existing tokens continue | auth_failure_contract | `f15_existing_tokens_continue_working` | :white_large_square: |
| failure-modes\.md § F15 | Login fails IdP unreachable | auth_failure_contract | `f15_login_fails_with_idp_unreachable` | :white_large_square: |
| failure-modes\.md § F15 | Cached discovery used | auth_failure_contract | `f15_cached_discovery_used` | :white_large_square: |
| failure-modes\.md § F16 | Corrupted cache fail-closed | auth_failure_contract | `f16_corrupted_cache_rejected_fail_closed` | :white_large_square: |
| failure-modes\.md § F16 | Re-login recreates cache | auth_failure_contract | `f16_re_login_recreates_cache` | :white_large_square: |
| failure-modes\.md § F17 | Stale discovery cleared | auth_failure_contract | `f17_stale_discovery_cleared_on_auth_failure` | :white_large_square: |
| failure-modes\.md § F17 | Manual override bypasses discovery | auth_failure_contract | `f17_manual_override_bypasses_discovery` | :white_large_square: |
| failure-modes\.md § F13 | Conflicting keys pause convergence | conflict_failure_contract | `f13_conflicting_keys_pause_convergence` | :white_large_square: |
| failure-modes\.md § F13 | Non-conflicting keys sync | conflict_failure_contract | `f13_non_conflicting_keys_sync_normally` | :white_large_square: |
| failure-modes\.md § F13 | Admin can accept local | conflict_failure_contract | `f13_admin_can_accept_local` | :white_large_square: |
| failure-modes\.md § F13 | Admin can accept journal | conflict_failure_contract | `f13_admin_can_accept_journal` | :white_large_square: |
| failure-modes\.md § F13 | Grace period journal-wins | conflict_failure_contract | `f13_grace_period_timeout_journal_wins` | :white_large_square: |
| failure-modes\.md § F13 | Overwritten changes logged | conflict_failure_contract | `f13_overwritten_changes_logged` | :white_large_square: |
| failure-modes\.md § F14 | Promote pauses on conflicts | conflict_failure_contract | `f14_promote_pauses_on_conflicts` | :white_large_square: |
| failure-modes\.md § F14 | Each conflict needs explicit ack | conflict_failure_contract | `f14_each_conflict_requires_explicit_ack` | :white_large_square: |
| failure-modes\.md § F14 | No conflicts proceeds | conflict_failure_contract | `f14_no_conflicts_promote_proceeds` | :white_large_square: |

## Contracts NOT covered (and why)

| Contract | Reason |
|---|---|
| E3 (single activation across domains) | Physical constraint -- cannot be unit-tested. Covered by multi-domain BDD scenarios. |
| E5 (dual-channel cert rotation atomicity) | Requires live gRPC channel swap. Covered by integration/E2E tests, not contract tests. |
| E6 (dual-channel rotation atomicity) | Requires live gRPC channel swap. Covered by integration/E2E tests, not contract tests. |
| A7 (cgroup enforcement) | Requires live cgroup v2 filesystem. Covered by integration tests on Linux. |
| A8 (eBPF observer) | Requires live eBPF programs. Covered by integration tests on Linux. |
| O1-O4 (observability invariants) | Infrastructure concerns. Covered by BDD scenarios and Grafana integration tests. |
| F10-F12 (Sovra/federation/overlay failures) | Cross-system failures. Covered by E2E tests. |
| Rate limiting (enrollment endpoint) | Infrastructure concern. Covered by BDD scenario "Enrollment endpoint is rate-limited". |
| Heartbeat timeout (Active to Inactive) | Requires timer-based state transition. Covered by BDD scenario. |
| Batch enrollment partial failure | Composition of individual RegisterNode contracts. Covered by BDD scenario. |

## Statistics

| Category | Files | Tests |
|---|---|---|
| Interface Contracts | 5 | 114 |
| Data Contracts | 2 | 37 |
| Event Contracts | 2 | 48 |
| Invariant Contracts | 7 | 104 |
| Failure Contracts | 5 | 45 |
| **Total** | **21** | **348** |
