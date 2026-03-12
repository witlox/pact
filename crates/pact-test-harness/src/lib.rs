//! `pact-test-harness` — shared test infrastructure for pact.
//!
//! Provides builders, fixtures, and mocks for cross-crate integration tests.

pub mod fixtures;
pub mod mocks;

/// Re-export builders for convenience.
pub mod builders {
    pub use crate::fixtures::builders::*;
}
