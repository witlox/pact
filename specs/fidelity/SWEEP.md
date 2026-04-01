# Sweep Plan

Status: COMPLETE
Started: 2026-04-01 (full re-sweep)
Completed: 2026-04-01

## Surface

| Type | Count | Assessed | Remaining |
|------|-------|----------|-----------|
| Feature files | 32 | 32 | 0 |
| BDD scenarios | 583 | 583 | 0 |
| Unit test modules | 81 | 81 | 0 |
| E2E test files | 13 | 13 | 0 |
| Traits | 15 | 15 | 0 |
| ADRs | 17 | 17 | 0 |

## Chunks (ordered by risk)

| # | Scope | Specs | Traits | Status | Session |
|---|-------|-------|--------|--------|---------|
| 1 | Raft consensus + journal | journal_operations, partition_resilience | — | DONE | 2026-04-01 |
| 2 | Auth + policy | auth_login, auth_logout, auth_token_refresh, cli_authentication, rbac_authorization, policy_evaluation | TokenValidator, PolicyEngine, OpaClient | DONE | 2026-04-01 |
| 3 | Node lifecycle | node_enrollment, boot_sequence, boot_config_streaming, platform_bootstrap | — | DONE | 2026-04-01 |
| 4 | Agent core | drift_detection, commit_window, emergency_mode, process_supervisor | ServiceManager, Observer | DONE | 2026-04-01 |
| 5 | Shell + exec + diag | shell_session, exec_endpoint, diag_retrieval | — | DONE | 2026-04-01 |
| 6 | Capabilities + isolation | hardware_detection, capability_reporting, resource_isolation | 5 capability backends | DONE | 2026-04-01 |
| 7 | Config + CLI | cli_commands, overlay_management, network_management | NetworkManager | DONE | 2026-04-01 |
| 8 | Integration + external | cross_context, workload_integration, federation, identity_mapping, agentic_api, observability, node-management-delegation | NodeManagementBackend, FederationSync | DONE | 2026-04-01 |
| 9 | Cross-cutting | ADRs, gaps, dead specs, orphan tests, feature flags | — | DONE | 2026-04-01 |
