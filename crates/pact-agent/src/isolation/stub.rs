//! Stub cgroup manager for non-Linux platforms (macOS development).
//!
//! Logs all operations but does not interact with the filesystem.
//! All operations succeed. Metrics return defaults.

use hpc_node::cgroup::slices;
use hpc_node::{CgroupError, CgroupHandle, CgroupManager, CgroupMetrics, ResourceLimits};
use tracing::{debug, info};

/// Stub cgroup manager for development on non-Linux platforms.
pub struct StubCgroupManager {
    /// Track created scopes for `is_scope_empty` (always returns true).
    scopes: std::sync::Mutex<std::collections::HashSet<String>>,
}

impl StubCgroupManager {
    #[must_use]
    pub fn new() -> Self {
        Self {
            scopes: std::sync::Mutex::new(std::collections::HashSet::new()),
        }
    }
}

impl Default for StubCgroupManager {
    fn default() -> Self {
        Self::new()
    }
}

impl CgroupManager for StubCgroupManager {
    fn create_hierarchy(&self) -> Result<(), CgroupError> {
        info!("stub: creating cgroup hierarchy (no-op)");
        debug!(
            "stub: would create {} {} {} {} {}",
            slices::PACT_ROOT,
            slices::PACT_INFRA,
            slices::PACT_NETWORK,
            slices::PACT_GPU,
            slices::WORKLOAD_ROOT,
        );
        Ok(())
    }

    fn create_scope(
        &self,
        parent_slice: &str,
        name: &str,
        limits: &ResourceLimits,
    ) -> Result<CgroupHandle, CgroupError> {
        let path = format!("{parent_slice}/{name}.scope");
        info!(
            path = %path,
            memory_max = ?limits.memory_max,
            cpu_weight = ?limits.cpu_weight,
            "stub: creating cgroup scope (no-op)"
        );
        self.scopes
            .lock()
            .expect("stub scope lock poisoned")
            .insert(path.clone());
        Ok(CgroupHandle { path })
    }

    fn destroy_scope(&self, handle: &CgroupHandle) -> Result<(), CgroupError> {
        info!(path = %handle.path, "stub: destroying cgroup scope (no-op)");
        self.scopes
            .lock()
            .expect("stub scope lock poisoned")
            .remove(&handle.path);
        Ok(())
    }

    fn read_metrics(&self, path: &str) -> Result<CgroupMetrics, CgroupError> {
        debug!(path = %path, "stub: reading cgroup metrics (returning defaults)");
        Ok(CgroupMetrics::default())
    }

    fn is_scope_empty(&self, handle: &CgroupHandle) -> Result<bool, CgroupError> {
        let exists = self
            .scopes
            .lock()
            .expect("stub scope lock poisoned")
            .contains(&handle.path);
        // Stub: scope is "empty" if it doesn't exist (was destroyed)
        Ok(!exists)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_create_hierarchy() {
        let mgr = StubCgroupManager::new();
        assert!(mgr.create_hierarchy().is_ok());
    }

    #[test]
    fn stub_scope_lifecycle() {
        let mgr = StubCgroupManager::new();
        mgr.create_hierarchy().unwrap();

        let handle = mgr
            .create_scope(slices::PACT_GPU, "nvidia", &ResourceLimits::default())
            .unwrap();
        assert_eq!(handle.path, "pact.slice/gpu.slice/nvidia.scope");

        // Scope exists → not empty
        assert!(!mgr.is_scope_empty(&handle).unwrap());

        // Destroy
        mgr.destroy_scope(&handle).unwrap();

        // Scope gone → empty
        assert!(mgr.is_scope_empty(&handle).unwrap());
    }

    #[test]
    fn stub_read_metrics_returns_defaults() {
        let mgr = StubCgroupManager::new();
        let metrics = mgr.read_metrics("any/path").unwrap();
        assert_eq!(metrics.memory_current, 0);
        assert_eq!(metrics.nr_processes, 0);
    }
}
