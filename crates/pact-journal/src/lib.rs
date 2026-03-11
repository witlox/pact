//! `pact-journal` — distributed immutable configuration log.
//!
//! Runs its own Raft group (independent from lattice) as the single source of
//! truth for declared HPC/AI cluster configuration state.
//!
//! See `docs/architecture/journal-design.md` for design documentation.

pub mod raft;

pub use raft::{JournalCommand, JournalResponse, JournalState, JournalTypeConfig};
