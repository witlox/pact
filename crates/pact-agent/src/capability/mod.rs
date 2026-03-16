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
    CapabilityReport, ConfigState, GpuCapability, MemoryCapability, MountPointInfo,
    NetworkCapability, SoftwareCapability, StorageCapability, SupervisorBackend, SupervisorStatus,
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
            network: detect_network(),
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

/// Detect storage capabilities from /proc/mounts (Linux) or defaults.
fn detect_storage() -> StorageCapability {
    let mounts = parse_proc_mounts();
    let tmpfs_bytes =
        mounts.iter().filter(|m| m.fs_type == "tmpfs").count() as u64 * 64 * 1024 * 1024; // estimate 64MB per tmpfs
    StorageCapability { tmpfs_bytes, mounts }
}

/// Parse /proc/mounts into MountPointInfo entries.
fn parse_proc_mounts() -> Vec<MountPointInfo> {
    let content = match std::fs::read_to_string("/proc/mounts") {
        Ok(c) => c,
        Err(_) => return vec![], // non-Linux or unreadable
    };
    content
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                Some(MountPointInfo {
                    source: parts[0].to_string(),
                    path: parts[1].to_string(),
                    fs_type: parts[2].to_string(),
                    available: true,
                })
            } else {
                None
            }
        })
        .collect()
}

/// Detect software capabilities from /proc/modules (Linux) or defaults.
fn detect_software() -> SoftwareCapability {
    let loaded_modules = parse_proc_modules();
    SoftwareCapability { loaded_modules, uenv_image: None, services: vec![] }
}

/// Detect network capabilities by checking for high-speed fabric interfaces.
fn detect_network() -> Option<NetworkCapability> {
    // Check for common HPC fabric types via /sys/class/infiniband or loaded modules
    let has_infiniband = std::path::Path::new("/sys/class/infiniband").exists();
    if has_infiniband {
        return Some(NetworkCapability {
            fabric_type: "InfiniBand".to_string(),
            bandwidth_bps: 200_000_000_000, // 200 Gbps default
            latency_us: 1.0,
        });
    }

    // Check for Slingshot (Cray/HPE)
    let modules = parse_proc_modules();
    if modules.iter().any(|m| m.starts_with("cxi") || m.starts_with("slingshot")) {
        return Some(NetworkCapability {
            fabric_type: "Slingshot".to_string(),
            bandwidth_bps: 200_000_000_000,
            latency_us: 0.5,
        });
    }

    // Default Ethernet
    None
}

/// Parse /proc/modules into a list of loaded module names.
fn parse_proc_modules() -> Vec<String> {
    let content = match std::fs::read_to_string("/proc/modules") {
        Ok(c) => c,
        Err(_) => return vec![], // non-Linux or unreadable
    };
    content.lines().filter_map(|line| line.split_whitespace().next().map(String::from)).collect()
}

// ---------------------------------------------------------------------------
// NVIDIA GPU backend (feature: nvidia)
// ---------------------------------------------------------------------------

/// GPU backend using `nvidia-smi` CLI to detect NVIDIA GPUs.
///
/// Runs `nvidia-smi --query-gpu=index,name,memory.total,pci.bus_id --format=csv,noheader,nounits`
/// and parses the CSV output.
#[cfg(feature = "nvidia")]
pub struct NvidiaSmiBackend;

#[cfg(feature = "nvidia")]
impl Default for NvidiaSmiBackend {
    fn default() -> Self {
        Self
    }
}

#[cfg(feature = "nvidia")]
impl NvidiaSmiBackend {
    pub fn new() -> Self {
        Self
    }

    /// Parse nvidia-smi CSV output into `GpuCapability` entries.
    ///
    /// Expected format per line: `index, name, memory_mib, pci_bus_id`
    /// e.g. `0, NVIDIA A100-SXM4-80GB, 81920, 00000000:3B:00.0`
    fn parse_nvidia_csv(output: &str) -> Vec<GpuCapability> {
        use pact_common::types::{GpuHealth, GpuVendor};

        let mut gpus = Vec::new();
        for line in output.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.splitn(4, ',').map(str::trim).collect();
            if parts.len() < 4 {
                continue;
            }
            let index = match parts[0].parse::<u32>() {
                Ok(i) => i,
                Err(_) => continue,
            };
            let model = parts[1].to_string();
            let memory_mib = match parts[2].parse::<u64>() {
                Ok(m) => m,
                Err(_) => continue,
            };
            let pci_bus_id = parts[3].to_string();

            gpus.push(GpuCapability {
                index,
                vendor: GpuVendor::Nvidia,
                model,
                memory_bytes: memory_mib * 1024 * 1024,
                health: GpuHealth::Healthy,
                pci_bus_id,
            });
        }
        gpus
    }
}

#[cfg(feature = "nvidia")]
#[async_trait]
impl GpuBackend for NvidiaSmiBackend {
    async fn detect(&self) -> anyhow::Result<Vec<GpuCapability>> {
        let output = tokio::process::Command::new("nvidia-smi")
            .args([
                "--query-gpu=index,name,memory.total,pci.bus_id",
                "--format=csv,noheader,nounits",
            ])
            .output()
            .await;

        match output {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                Ok(Self::parse_nvidia_csv(&stdout))
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                anyhow::bail!("nvidia-smi failed (exit {}): {}", out.status, stderr.trim())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // nvidia-smi not found — no NVIDIA GPUs or driver not installed
                Ok(vec![])
            }
            Err(e) => Err(e.into()),
        }
    }
}

// ---------------------------------------------------------------------------
// AMD GPU backend (feature: amd)
// ---------------------------------------------------------------------------

/// GPU backend using `rocm-smi` CLI to detect AMD GPUs.
///
/// Runs `rocm-smi --showid --showproductname --showmeminfo vram --csv`
/// and parses the CSV output.
#[cfg(feature = "amd")]
pub struct RocmSmiBackend;

#[cfg(feature = "amd")]
impl Default for RocmSmiBackend {
    fn default() -> Self {
        Self
    }
}

#[cfg(feature = "amd")]
impl RocmSmiBackend {
    pub fn new() -> Self {
        Self
    }

    /// Parse rocm-smi CSV output into `GpuCapability` entries.
    ///
    /// rocm-smi CSV output typically has a header row followed by data rows.
    /// Columns vary but we look for: device index, card series/model, VRAM total.
    fn parse_rocm_csv(output: &str) -> Vec<GpuCapability> {
        use pact_common::types::{GpuHealth, GpuVendor};

        let mut gpus = Vec::new();
        let mut lines = output.lines();

        // Skip header line
        let header = match lines.next() {
            Some(h) => h,
            None => return gpus,
        };

        // Find column indices from header
        let headers: Vec<&str> = header.split(',').map(str::trim).collect();
        let device_col = headers.iter().position(|h| h.contains("device"));
        let model_col = headers.iter().position(|h| {
            h.contains("Card series") || h.contains("Card Series") || h.contains("product")
        });
        let vram_col = headers.iter().position(|h| h.contains("VRAM Total") || h.contains("vram"));
        let id_col = headers.iter().position(|h| h.contains("GPU ID") || h.contains("Bus"));

        for line in lines {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.split(',').map(str::trim).collect();

            let index = device_col
                .and_then(|c| parts.get(c))
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(gpus.len() as u32);

            let model = model_col
                .and_then(|c| parts.get(c))
                .map_or_else(|| "Unknown AMD GPU".to_string(), ToString::to_string);

            // VRAM is typically in bytes from rocm-smi --csv
            let memory_bytes = vram_col
                .and_then(|c| parts.get(c))
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);

            let pci_bus_id =
                id_col.and_then(|c| parts.get(c)).map(ToString::to_string).unwrap_or_default();

            gpus.push(GpuCapability {
                index,
                vendor: GpuVendor::Amd,
                model,
                memory_bytes,
                health: GpuHealth::Healthy,
                pci_bus_id,
            });
        }
        gpus
    }
}

#[cfg(feature = "amd")]
#[async_trait]
impl GpuBackend for RocmSmiBackend {
    async fn detect(&self) -> anyhow::Result<Vec<GpuCapability>> {
        let output = tokio::process::Command::new("rocm-smi")
            .args(["--showid", "--showproductname", "--showmeminfo", "vram", "--csv"])
            .output()
            .await;

        match output {
            Ok(out) if out.status.success() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                Ok(Self::parse_rocm_csv(&stdout))
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                anyhow::bail!("rocm-smi failed (exit {}): {}", out.status, stderr.trim())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // rocm-smi not found — no AMD GPUs or ROCm not installed
                Ok(vec![])
            }
            Err(e) => Err(e.into()),
        }
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
        // Network detection is platform-dependent (may find InfiniBand/Slingshot on HPC nodes)
        // so we don't assert is_none — just verify the report was generated
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

    // --- NVIDIA CSV parsing tests ---

    #[cfg(feature = "nvidia")]
    #[test]
    fn parse_nvidia_csv_single_gpu() {
        let csv = "0, NVIDIA A100-SXM4-80GB, 81920, 00000000:3B:00.0\n";
        let gpus = NvidiaSmiBackend::parse_nvidia_csv(csv);
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].index, 0);
        assert_eq!(gpus[0].vendor, GpuVendor::Nvidia);
        assert_eq!(gpus[0].model, "NVIDIA A100-SXM4-80GB");
        assert_eq!(gpus[0].memory_bytes, 81920 * 1024 * 1024);
        assert_eq!(gpus[0].pci_bus_id, "00000000:3B:00.0");
        assert_eq!(gpus[0].health, GpuHealth::Healthy);
    }

    #[cfg(feature = "nvidia")]
    #[test]
    fn parse_nvidia_csv_multiple_gpus() {
        let csv = "\
0, NVIDIA A100-SXM4-80GB, 81920, 00000000:3B:00.0
1, NVIDIA A100-SXM4-80GB, 81920, 00000000:86:00.0
2, NVIDIA A100-SXM4-80GB, 81920, 00000000:AF:00.0
3, NVIDIA A100-SXM4-80GB, 81920, 00000000:D8:00.0
";
        let gpus = NvidiaSmiBackend::parse_nvidia_csv(csv);
        assert_eq!(gpus.len(), 4);
        assert_eq!(gpus[3].index, 3);
        assert_eq!(gpus[3].pci_bus_id, "00000000:D8:00.0");
    }

    #[cfg(feature = "nvidia")]
    #[test]
    fn parse_nvidia_csv_empty_output() {
        let gpus = NvidiaSmiBackend::parse_nvidia_csv("");
        assert!(gpus.is_empty());
    }

    #[cfg(feature = "nvidia")]
    #[test]
    fn parse_nvidia_csv_malformed_line_skipped() {
        let csv = "not,enough,columns\n0, A100, 81920, 00:00.0\n";
        let gpus = NvidiaSmiBackend::parse_nvidia_csv(csv);
        // First line has 4 columns but "not" is not a valid u32 → skipped
        // Second line is valid
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].model, "A100");
    }

    // --- AMD CSV parsing tests ---

    #[cfg(feature = "amd")]
    #[test]
    fn parse_rocm_csv_single_gpu() {
        let csv = "device, Card series, VRAM Total, GPU ID\n\
                   0, Instinct MI300X, 206158430208, 0000:c1:00.0\n";
        let gpus = RocmSmiBackend::parse_rocm_csv(csv);
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].index, 0);
        assert_eq!(gpus[0].vendor, GpuVendor::Amd);
        assert_eq!(gpus[0].model, "Instinct MI300X");
        assert_eq!(gpus[0].memory_bytes, 206_158_430_208);
        assert_eq!(gpus[0].pci_bus_id, "0000:c1:00.0");
    }

    #[cfg(feature = "amd")]
    #[test]
    fn parse_rocm_csv_multiple_gpus() {
        let csv = "device, Card series, VRAM Total, GPU ID\n\
                   0, Instinct MI300X, 206158430208, 0000:c1:00.0\n\
                   1, Instinct MI300X, 206158430208, 0000:c6:00.0\n";
        let gpus = RocmSmiBackend::parse_rocm_csv(csv);
        assert_eq!(gpus.len(), 2);
        assert_eq!(gpus[1].index, 1);
    }

    #[cfg(feature = "amd")]
    #[test]
    fn parse_rocm_csv_empty_output() {
        let gpus = RocmSmiBackend::parse_rocm_csv("");
        assert!(gpus.is_empty());
    }

    #[cfg(feature = "amd")]
    #[test]
    fn parse_rocm_csv_header_only() {
        let csv = "device, Card series, VRAM Total, GPU ID\n";
        let gpus = RocmSmiBackend::parse_rocm_csv(csv);
        assert!(gpus.is_empty());
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
