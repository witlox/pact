//! Resource isolation — cgroup v2 hierarchy management.
//!
//! Implements [`hpc_node::CgroupManager`] for direct cgroup v2 filesystem
//! operations when running in PactSupervisor mode. On non-Linux platforms
//! (macOS development), a stub implementation is provided.
//!
//! # cgroup hierarchy
//!
//! ```text
//! /sys/fs/cgroup/
//! ├── pact.slice/               # Owned by pact (system services)
//! │   ├── infra.slice/          # chronyd, dbus-daemon, rasdaemon
//! │   ├── network.slice/        # cxi_rh instances
//! │   ├── gpu.slice/            # nvidia-persistenced, nv-hostengine
//! │   └── audit.slice/          # auditd, audit-forwarder
//! └── workload.slice/           # Owned by lattice (allocations)
//! ```
//!
//! # Invariants enforced
//!
//! - RI1: Exclusive slice ownership (checked on `create_scope`)
//! - RI2: Every supervised process has a cgroup scope
//! - RI4: pact-agent OOM protection (`OOMScoreAdj=-1000`)
//! - RI5: Cleanup callback on failure
//! - RI6: Shared read across all slices
//! - PS3: `cgroup.kill` for immediate child cleanup

#[cfg(target_os = "linux")]
mod linux;
mod stub;

#[cfg(target_os = "linux")]
pub use linux::LinuxCgroupManager;
pub use stub::StubCgroupManager;

use hpc_node::CgroupManager;

/// Create the appropriate cgroup manager for the current platform.
///
/// On Linux: creates a real cgroup v2 manager.
/// On other platforms (macOS dev): creates a stub that logs operations.
#[must_use]
pub fn create_cgroup_manager(cgroup_root: &str) -> Box<dyn CgroupManager> {
    #[cfg(target_os = "linux")]
    {
        Box::new(LinuxCgroupManager::new(cgroup_root))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = cgroup_root;
        Box::new(StubCgroupManager::new())
    }
}

/// Set OOM score adjustment for the current process.
///
/// On Linux: writes to `/proc/self/oom_score_adj`.
/// Invariant RI4: pact-agent runs with `OOMScoreAdj=-1000`.
pub fn protect_from_oom() -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        std::fs::write("/proc/self/oom_score_adj", "-1000")?;
        tracing::info!("OOM protection set: oom_score_adj=-1000");
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        tracing::debug!("OOM protection: skipped (not Linux)");
        Ok(())
    }
}
