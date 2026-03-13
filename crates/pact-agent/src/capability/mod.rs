//! Capability reporter — hardware detection and reporting.
//!
//! Detects GPU, memory, storage, network, and software capabilities.
//! Writes `CapabilityReport` to:
//! - tmpfs manifest at `/run/pact/capability.json`
//! - unix socket for lattice-node-agent
//!
//! GPU detection is vendor-neutral behind the `GpuBackend` trait:
//! - NVIDIA: NVML (feature `nvidia`)
//! - AMD: ROCm SMI (feature `amd`)
//! - MockGpuBackend for macOS development

use async_trait::async_trait;
use chrono::Utc;
use uuid::Uuid;

use pact_common::types::{
    CapabilityReport, ConfigState, GpuCapability, MemoryCapability, SoftwareCapability,
    StorageCapability, SupervisorBackend, SupervisorStatus,
};

/// Trait for GPU detection backends.
#[async_trait]
pub trait GpuBackend: Send + Sync {
    /// Detect GPUs and return their capabilities.
    async fn detect(&self) -> anyhow::Result<Vec<GpuCapability>>;
}

/// Mock GPU backend for development/testing.
pub struct MockGpuBackend {
    gpus: Vec<GpuCapability>,
}

impl MockGpuBackend {
    pub fn new() -> Self {
        Self { gpus: vec![] }
    }

    /// Create a mock with pre-configured GPUs.
    pub fn with_gpus(gpus: Vec<GpuCapability>) -> Self {
        Self { gpus }
    }
}

impl Default for MockGpuBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl GpuBackend for MockGpuBackend {
    async fn detect(&self) -> anyhow::Result<Vec<GpuCapability>> {
        Ok(self.gpus.clone())
    }
}

/// Builds a `CapabilityReport` by querying system capabilities.
pub struct CapabilityReporter {
    node_id: String,
    gpu_backend: Box<dyn GpuBackend>,
}

impl CapabilityReporter {
    pub fn new(node_id: String, gpu_backend: Box<dyn GpuBackend>) -> Self {
        Self { node_id, gpu_backend }
    }

    /// Generate a full capability report.
    pub async fn report(&self) -> anyhow::Result<CapabilityReport> {
        let gpus = self.gpu_backend.detect().await?;

        Ok(CapabilityReport {
            node_id: self.node_id.clone(),
            timestamp: Utc::now(),
            report_id: Uuid::new_v4(),
            gpus,
            memory: detect_memory(),
            network: None, // TODO: detect Slingshot/InfiniBand/Ethernet
            storage: detect_storage(),
            software: detect_software(),
            config_state: ConfigState::ObserveOnly,
            drift_summary: None,
            emergency: None,
            supervisor_status: SupervisorStatus {
                backend: SupervisorBackend::Pact,
                services_declared: 0,
                services_running: 0,
                services_failed: 0,
            },
        })
    }

    /// Write report to JSON file (tmpfs manifest).
    pub async fn write_manifest(
        &self,
        report: &CapabilityReport,
        path: &std::path::Path,
    ) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let json = serde_json::to_string_pretty(report)?;
        tokio::fs::write(path, json).await?;
        Ok(())
    }
}

/// Detect system memory.
fn detect_memory() -> MemoryCapability {
    #[cfg(target_os = "linux")]
    {
        if let Ok(meminfo) = std::fs::read_to_string("/proc/meminfo") {
            return parse_meminfo(&meminfo);
        }
    }
    MemoryCapability { total_bytes: 0, available_bytes: 0, numa_nodes: 1 }
}

/// Parse /proc/meminfo content into MemoryCapability.
///
/// Extracted for testability — this is the real parsing logic.
#[cfg(any(test, target_os = "linux"))]
fn parse_meminfo(content: &str) -> MemoryCapability {
    let mut total = 0u64;
    let mut available = 0u64;
    for line in content.lines() {
        if line.starts_with("MemTotal:") {
            if let Some(kb) = parse_meminfo_kb(line) {
                total = kb * 1024;
            }
        } else if line.starts_with("MemAvailable:") {
            if let Some(kb) = parse_meminfo_kb(line) {
                available = kb * 1024;
            }
        }
    }
    MemoryCapability { total_bytes: total, available_bytes: available, numa_nodes: 1 }
}

/// Parse a single line from /proc/meminfo, extracting the kB value.
#[cfg(any(test, target_os = "linux"))]
fn parse_meminfo_kb(line: &str) -> Option<u64> {
    line.split_whitespace().nth(1)?.parse().ok()
}

/// Detect storage capabilities.
fn detect_storage() -> StorageCapability {
    StorageCapability {
        tmpfs_bytes: 0,
        mounts: vec![], // TODO: detect from /proc/mounts
    }
}

/// Detect software capabilities.
fn detect_software() -> SoftwareCapability {
    SoftwareCapability {
        loaded_modules: vec![], // TODO: detect from /proc/modules
        uenv_image: None,
        services: vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pact_common::types::{GpuHealth, GpuVendor};

    fn test_gpu() -> GpuCapability {
        GpuCapability {
            index: 0,
            vendor: GpuVendor::Nvidia,
            model: "A100-SXM4-80GB".into(),
            memory_bytes: 80 * 1024 * 1024 * 1024,
            health: GpuHealth::Healthy,
            pci_bus_id: "0000:3b:00.0".into(),
        }
    }

    // --- parse_meminfo_kb tests (the real parsing logic) ---

    #[test]
    fn parse_meminfo_kb_standard_line() {
        assert_eq!(parse_meminfo_kb("MemTotal:       65536000 kB"), Some(65_536_000));
    }

    #[test]
    fn parse_meminfo_kb_with_varying_whitespace() {
        assert_eq!(parse_meminfo_kb("MemAvailable:    1234567 kB"), Some(1_234_567));
        assert_eq!(parse_meminfo_kb("Buffers:         42 kB"), Some(42));
    }

    #[test]
    fn parse_meminfo_kb_malformed_returns_none() {
        assert_eq!(parse_meminfo_kb("MemTotal:"), None);
        assert_eq!(parse_meminfo_kb("MemTotal: not_a_number kB"), None);
        assert_eq!(parse_meminfo_kb(""), None);
    }

    // --- parse_meminfo tests (full /proc/meminfo parsing) ---

    #[test]
    fn parse_meminfo_real_format() {
        let content = "\
MemTotal:       65797240 kB
MemFree:        12345678 kB
MemAvailable:   50000000 kB
Buffers:          234567 kB
Cached:         20000000 kB
SwapCached:            0 kB
";
        let mem = parse_meminfo(content);
        assert_eq!(mem.total_bytes, 65_797_240 * 1024);
        assert_eq!(mem.available_bytes, 50_000_000 * 1024);
        assert_eq!(mem.numa_nodes, 1);
    }

    #[test]
    fn parse_meminfo_missing_available() {
        // Some kernels might not have MemAvailable
        let content = "MemTotal:       16000000 kB\nMemFree:        8000000 kB\n";
        let mem = parse_meminfo(content);
        assert_eq!(mem.total_bytes, 16_000_000 * 1024);
        assert_eq!(mem.available_bytes, 0); // Not present → 0
    }

    #[test]
    fn parse_meminfo_empty_input() {
        let mem = parse_meminfo("");
        assert_eq!(mem.total_bytes, 0);
        assert_eq!(mem.available_bytes, 0);
    }

    #[test]
    fn parse_meminfo_garbage_input() {
        let mem = parse_meminfo("this is not meminfo\nrandom garbage\n");
        assert_eq!(mem.total_bytes, 0);
        assert_eq!(mem.available_bytes, 0);
    }

    // --- CapabilityReport structure tests ---

    #[tokio::test]
    async fn report_with_gpus_includes_all_fields() {
        let reporter = CapabilityReporter::new(
            "node-001".into(),
            Box::new(MockGpuBackend::with_gpus(vec![test_gpu()])),
        );
        let report = reporter.report().await.unwrap();

        assert_eq!(report.node_id, "node-001");
        assert_eq!(report.gpus.len(), 1);
        assert_eq!(report.gpus[0].model, "A100-SXM4-80GB");
        assert_eq!(report.gpus[0].vendor, GpuVendor::Nvidia);
        assert_eq!(report.gpus[0].health, GpuHealth::Healthy);
        assert_eq!(report.gpus[0].memory_bytes, 80 * 1024 * 1024 * 1024);
        assert_eq!(report.config_state, ConfigState::ObserveOnly);
        assert!(report.network.is_none());
        assert!(report.drift_summary.is_none());
        assert!(report.emergency.is_none());

        // Supervisor status should have zero counts (no services started)
        assert_eq!(report.supervisor_status.backend, SupervisorBackend::Pact);
        assert_eq!(report.supervisor_status.services_declared, 0);
        assert_eq!(report.supervisor_status.services_running, 0);
        assert_eq!(report.supervisor_status.services_failed, 0);
    }

    #[tokio::test]
    async fn report_without_gpus() {
        let reporter = CapabilityReporter::new("node-002".into(), Box::new(MockGpuBackend::new()));
        let report = reporter.report().await.unwrap();

        assert_eq!(report.node_id, "node-002");
        assert!(report.gpus.is_empty());
        // Report should still have a unique ID and timestamp
        assert!(!report.report_id.is_nil());
    }

    #[tokio::test]
    async fn report_with_multiple_gpus() {
        let gpus = vec![
            GpuCapability {
                index: 0,
                vendor: GpuVendor::Nvidia,
                model: "A100-SXM4-80GB".into(),
                memory_bytes: 80 * 1024 * 1024 * 1024,
                health: GpuHealth::Healthy,
                pci_bus_id: "0000:3b:00.0".into(),
            },
            GpuCapability {
                index: 1,
                vendor: GpuVendor::Nvidia,
                model: "A100-SXM4-80GB".into(),
                memory_bytes: 80 * 1024 * 1024 * 1024,
                health: GpuHealth::Degraded,
                pci_bus_id: "0000:86:00.0".into(),
            },
        ];
        let reporter =
            CapabilityReporter::new("gpu-node".into(), Box::new(MockGpuBackend::with_gpus(gpus)));
        let report = reporter.report().await.unwrap();
        assert_eq!(report.gpus.len(), 2);
        assert_eq!(report.gpus[1].health, GpuHealth::Degraded);
        assert_eq!(report.gpus[1].pci_bus_id, "0000:86:00.0");
    }

    // --- Manifest write/read roundtrip ---

    #[tokio::test]
    async fn write_manifest_roundtrip_preserves_all_fields() {
        let reporter = CapabilityReporter::new(
            "node-001".into(),
            Box::new(MockGpuBackend::with_gpus(vec![test_gpu()])),
        );
        let report = reporter.report().await.unwrap();

        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("subdir/capability.json"); // tests parent dir creation
        reporter.write_manifest(&report, &path).await.unwrap();

        let contents = tokio::fs::read_to_string(&path).await.unwrap();
        let parsed: CapabilityReport = serde_json::from_str(&contents).unwrap();

        assert_eq!(parsed.node_id, report.node_id);
        assert_eq!(parsed.report_id, report.report_id);
        assert_eq!(parsed.gpus.len(), 1);
        assert_eq!(parsed.gpus[0].model, "A100-SXM4-80GB");
        assert_eq!(parsed.gpus[0].memory_bytes, 80 * 1024 * 1024 * 1024);
        assert_eq!(parsed.config_state, ConfigState::ObserveOnly);
        assert_eq!(parsed.supervisor_status.backend, SupervisorBackend::Pact);
    }

    // --- Failing GPU backend ---

    struct FailingGpuBackend;

    #[async_trait]
    impl GpuBackend for FailingGpuBackend {
        async fn detect(&self) -> anyhow::Result<Vec<GpuCapability>> {
            anyhow::bail!("NVML init failed: driver not loaded")
        }
    }

    #[tokio::test]
    async fn gpu_detection_failure_propagates() {
        let reporter = CapabilityReporter::new("node-err".into(), Box::new(FailingGpuBackend));
        let result = reporter.report().await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("NVML init failed"));
    }
}
