//! Step definition modules for BDD acceptance tests.
//!
//! Each module registers steps for a specific domain area.
//! cucumber-rs macros register globally via the World type.
//!
//! Modules without real crate code behind them are deliberately absent —
//! unmatched scenarios show as "skipped" until real implementations exist.

pub mod helpers;

mod auth;
mod boot; // Boot sequence + boot config streaming
mod capability; // CapabilityReporter + MockGpuBackend
mod cli; // CLI formatting + exit codes + delegation
mod commit_window; // CommitWindowManager
mod diag; // Diagnostic log retrieval + validation
mod drift; // DriftEvaluator
mod emergency; // EmergencyManager
mod enrollment; // Node enrollment, domain membership, certificate lifecycle
mod federation;
mod identity_mapping; // UidMap + identity assignment
mod journal; // JournalState::apply_command()
mod mcp; // MCP tools: all_tools() + dispatch_tool()
mod network; // Network management: interface config, netlink, MTU
mod observability; // Prometheus metrics + health + Loki events
mod overlay; // Overlay management + staleness + promote/conflict
mod partition; // ConflictManager + cached config/policy
mod policy; // RbacEngine + DefaultPolicyEngine + RBAC authorization
mod resource_isolation; // cgroup management
mod shell; // ShellServer + WhitelistManager + SessionManager + execute_command
mod supervisor; // PactSupervisor + ServiceManager
mod workload_integration; // Namespace handoff + mount refcounting // FederationState + MockFederationSync // hpc-auth: login, logout, token refresh, cache, multi-server
