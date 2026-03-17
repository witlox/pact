//! `pact-journal` — distributed immutable configuration log.
//!
//! Runs its own Raft group (independent from lattice) as the single source of
//! truth for declared HPC/AI cluster configuration state.
//!
//! See `docs/architecture/journal-design.md` for design documentation.

pub mod auth;
pub mod boot_service;
pub mod ca;
pub mod enrollment_service;
pub mod heartbeat;
pub mod policy_service;
pub mod raft;
pub mod rate_limiter;
pub mod service;
pub mod telemetry;

pub use raft::{
    ConflictEntry, HomogeneityWarning, JournalCommand, JournalResponse, JournalState,
    JournalTypeConfig,
};
