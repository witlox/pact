//! `pact-agent` — per-node init system, process supervisor, and shell server.
//!
//! Core subsystems:
//! - **supervisor**: Process lifecycle management (PactSupervisor or SystemdBackend)
//! - **observer**: State change detection (eBPF, inotify, netlink)
//! - **drift**: Drift vector computation and evaluation
//! - **commit**: Commit window management with optimistic concurrency
//! - **capability**: Hardware capability reporting (GPU, memory, network)
//! - **emergency**: Emergency mode management
//! - **shell**: Exec and interactive shell (ADR-007: replaces SSH)
//!
//! See `docs/architecture/agent-design.md` for design documentation.

pub mod boot;
pub mod capability;
pub mod commit;
pub mod conflict;
pub mod drift;
pub mod emergency;
pub mod journal_client;
pub mod observer;
pub mod shell;
pub mod subscription;
pub mod supervisor;
