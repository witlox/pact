//! Raft state machine for the pact journal.
//!
//! Defines `JournalState` (the application state), `JournalCommand` (the Raft
//! command type), `JournalResponse` (the response type), and the openraft
//! `TypeConfig` declaration.

pub mod state;
pub mod types;

pub use state::{ConflictEntry, HomogeneityWarning, JournalState};
pub use types::{JournalCommand, JournalResponse, JournalTypeConfig};
