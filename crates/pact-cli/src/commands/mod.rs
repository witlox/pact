//! CLI commands for pact.
//!
//! Each subcommand maps to a gRPC call or local operation.
//! Commands handle argument parsing, gRPC client creation,
//! request construction, and output formatting.

pub mod apply;
pub mod approve;
pub mod blacklist;
pub mod commit;
pub mod config;
pub mod delegate;
pub mod diff;
pub mod emergency;
pub mod exec;
pub mod execute;
pub mod group;
pub mod log;
pub mod node;
pub mod openchami;
pub mod promote;
pub mod rollback;
pub mod service;
pub mod status;

pub use config::CliConfig;
