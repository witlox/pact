//! `pact-cli` — CLI for pact configuration management and admin operations.
//!
//! Commands map to gRPC calls against pact-journal and pact-agent services.
//! Every operation is authenticated via OIDC, authorized via RBAC, and logged.
//!
//! See `docs/architecture/cli-design.md` for the full command reference.

pub mod commands;
