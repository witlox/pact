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
mod policy; // RbacEngine + DefaultPolicyEngine
