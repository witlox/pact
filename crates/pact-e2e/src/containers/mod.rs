//! Container definitions for e2e integration testing.
//!
//! Each module provides a testcontainers [`Image`] implementation for a
//! specific service used by pact infrastructure.

pub mod loki;
pub mod opa;
pub mod prometheus;
pub mod raft_cluster;
pub mod spire;
