//! Step definition modules for BDD acceptance tests.
//!
//! Each module registers steps for a specific domain area.
//! cucumber-rs macros register globally via the World type.
//!
//! Modules without real crate code behind them are deliberately absent —
//! unmatched scenarios show as "skipped" until real implementations exist.

pub(crate) mod helpers;

mod journal; // JournalState::apply_command()
mod drift; // DriftEvaluator
mod commit_window; // CommitWindowManager
mod emergency; // EmergencyManager
mod policy; // RbacEngine + DefaultPolicyEngine + RBAC authorization
mod capability; // CapabilityReporter + MockGpuBackend
mod supervisor; // PactSupervisor + ServiceManager
mod shell; // ShellServer + WhitelistManager + SessionManager + execute_command
mod partition; // ConflictManager + cached config/policy
mod boot; // Boot sequence + boot config streaming
mod overlay; // Overlay management + staleness + promote/conflict
mod cli; // CLI formatting + exit codes + delegation
mod mcp; // MCP tools: all_tools() + dispatch_tool()
mod observability; // Prometheus metrics + health + Loki events
mod federation; // FederationState + MockFederationSync
