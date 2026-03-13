//! End-to-end integration test infrastructure for pact.
//!
//! Uses testcontainers to spin up real OPA, Prometheus, Loki, and multi-node
//! pact-journal Raft clusters for integration testing that goes beyond unit
//! and BDD acceptance tests.
//!
//! Run with: `cargo test -p pact-e2e -- --test-threads=1`
//! Or:       `just test-e2e`

pub mod containers;
