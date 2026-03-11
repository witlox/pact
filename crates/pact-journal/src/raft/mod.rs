//! Raft state machine for the pact journal.
//!
//! Defines `JournalState` (the application state), `JournalCommand` (the Raft
//! command type), `JournalResponse` (the response type), and the openraft
//! `TypeConfig` declaration.

mod state;
mod types;

pub use state::JournalState;
pub use types::{JournalCommand, JournalResponse, JournalTypeConfig};
