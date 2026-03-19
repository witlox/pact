//! CPU capability detection backend.
//!
//! Detects CPU architecture, model, core counts, frequencies, NUMA topology,
//! and ISA features from `/proc/cpuinfo` and sysfs on Linux.

use async_trait::async_trait;
use pact_common::types::{CpuArchitecture, CpuCapability};

/// Trait for CPU detection backends.
#[async_trait]
pub trait CpuBackend: Send + Sync {
    /// Detect CPU capabilities and return a [`CpuCapability`].
    async fn detect(&self) -> anyhow::Result<CpuCapability>;
}

/// Linux CPU backend — reads `/proc/cpuinfo` and sysfs.
pub struct LinuxCpuBackend;

impl LinuxCpuBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LinuxCpuBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CpuBackend for LinuxCpuBackend {
    async fn detect(&self) -> anyhow::Result<CpuCapability> {
        let cpuinfo = tokio::fs::read_to_string("/proc/cpuinfo").await.unwrap_or_default();

        let arch = detect_architecture();
        let (model, features, physical_cores, logical_cores) = parse_cpuinfo(&cpuinfo);
        let (base_freq, max_freq) = detect_frequency().await;
        let numa_nodes = detect_numa_nodes().await;
        let cache_l3 = detect_l3_cache().await;

        Ok(CpuCapability {
            architecture: arch,
            model,
            physical_cores,
            logical_cores,
            base_frequency_mhz: base_freq,
            max_frequency_mhz: max_freq,
            features,
            numa_nodes,
            cache_l3_bytes: cache_l3,
        })
    }
}

/// Mock CPU backend for development/testing.
pub struct MockCpuBackend {
    cpu: CpuCapability,
}

impl MockCpuBackend {
    pub fn new() -> Self {
        Self { cpu: CpuCapability::default() }
    }

    /// Create a mock with pre-configured CPU capability.
    pub fn with_cpu(cpu: CpuCapability) -> Self {
        Self { cpu }
    }
}

impl Default for MockCpuBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CpuBackend for MockCpuBackend {
    async fn detect(&self) -> anyhow::Result<CpuCapability> {
        Ok(self.cpu.clone())
    }
}

/// Detect CPU architecture from `std::env::consts::ARCH`.
fn detect_architecture() -> CpuArchitecture {
    match std::env::consts::ARCH {
        "x86_64" => CpuArchitecture::X86_64,
        "aarch64" => CpuArchitecture::Aarch64,
        _ => CpuArchitecture::Unknown,
    }
}

/// Parse `/proc/cpuinfo` content.
///
/// Returns `(model, features, physical_cores, logical_cores)`.
///
/// Handles both x86_64 format (`model name`, `flags`, `physical id`, `processor`)
/// and aarch64 format (`CPU implementer` + `CPU part`, `Features`, `processor`).
fn parse_cpuinfo(content: &str) -> (String, Vec<String>, u32, u32) {
    let mut model = String::new();
    let mut features = Vec::new();
    let mut logical_cores: u32 = 0;
    let mut physical_ids = std::collections::HashSet::new();
    let mut cores_per_socket: u32 = 0;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim();
            let value = value.trim();

            match key {
                // x86_64: "model name"
                "model name" if model.is_empty() => {
                    model = value.to_string();
                }
                // aarch64 fallback: construct model from implementer + part
                "CPU implementer" if model.is_empty() => {
                    model = format!("ARM implementer {value}");
                }
                "CPU part" if model.starts_with("ARM implementer") => {
                    model = format!("{model} part {value}");
                }
                // x86_64: "flags", aarch64: "Features"
                "flags" | "Features" if features.is_empty() => {
                    features = value.split_whitespace().map(String::from).collect();
                }
                // Count logical cores
                "processor" => {
                    if value.parse::<u32>().is_ok() {
                        logical_cores += 1;
                    }
                }
                // x86_64 physical core counting
                "physical id" => {
                    if let Ok(id) = value.parse::<u32>() {
                        physical_ids.insert(id);
                    }
                }
                "cpu cores" if cores_per_socket == 0 => {
                    if let Ok(c) = value.parse::<u32>() {
                        cores_per_socket = c;
                    }
                }
                _ => {}
            }
        }
    }

    // Compute physical cores
    let physical_cores = if !physical_ids.is_empty() && cores_per_socket > 0 {
        physical_ids.len() as u32 * cores_per_socket
    } else if logical_cores > 0 {
        // aarch64 or single-socket without physical id: assume no SMT
        logical_cores
    } else {
        0
    };

    (model, features, physical_cores, logical_cores)
}

/// Read CPU frequency from sysfs, returning `(base_mhz, max_mhz)`.
///
/// Falls back to `/proc/cpuinfo` "cpu MHz" if sysfs is unavailable.
async fn detect_frequency() -> (u32, u32) {
    // Try sysfs base_frequency (kHz)
    let base = read_sysfs_khz("/sys/devices/system/cpu/cpu0/cpufreq/base_frequency")
        .await
        .or(read_sysfs_khz("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_min_freq").await);

    // Try sysfs max frequency (kHz)
    let max = read_sysfs_khz("/sys/devices/system/cpu/cpu0/cpufreq/cpuinfo_max_freq").await;

    // Fallback: parse /proc/cpuinfo "cpu MHz"
    let fallback_mhz =
        if base.is_none() || max.is_none() { read_cpuinfo_mhz().await } else { None };

    let base_mhz = base.map(|khz| (khz / 1000) as u32).or(fallback_mhz).unwrap_or(0);
    let max_mhz = max.map(|khz| (khz / 1000) as u32).or(fallback_mhz).unwrap_or(0);

    (base_mhz, max_mhz)
}

/// Read a sysfs file containing a kHz value.
async fn read_sysfs_khz(path: &str) -> Option<u64> {
    let content = tokio::fs::read_to_string(path).await.ok()?;
    content.trim().parse::<u64>().ok()
}

/// Read "cpu MHz" from `/proc/cpuinfo` as fallback.
async fn read_cpuinfo_mhz() -> Option<u32> {
    let content = tokio::fs::read_to_string("/proc/cpuinfo").await.ok()?;
    for line in content.lines() {
        if line.starts_with("cpu MHz") {
            if let Some((_, value)) = line.split_once(':') {
                if let Ok(mhz) = value.trim().parse::<f64>() {
                    return Some(mhz as u32);
                }
            }
        }
    }
    None
}

/// Count NUMA nodes from sysfs.
async fn detect_numa_nodes() -> u32 {
    let path = std::path::Path::new("/sys/devices/system/node");
    if !path.exists() {
        return 1;
    }

    let mut count = 0u32;
    if let Ok(mut entries) = tokio::fs::read_dir(path).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with("node") && name[4..].parse::<u32>().is_ok() {
                    count += 1;
                }
            }
        }
    }

    if count == 0 {
        1
    } else {
        count
    }
}

/// Detect L3 cache size from sysfs.
async fn detect_l3_cache() -> u64 {
    let path = "/sys/devices/system/cpu/cpu0/cache/index3/size";
    if let Ok(content) = tokio::fs::read_to_string(path).await {
        return parse_cache_size(content.trim());
    }
    0
}

/// Parse cache size strings like "32768K" or "32M" into bytes.
fn parse_cache_size(s: &str) -> u64 {
    let s = s.trim();
    if s.is_empty() {
        return 0;
    }
    if let Some(kb) = s.strip_suffix('K') {
        kb.parse::<u64>().unwrap_or(0) * 1024
    } else if let Some(mb) = s.strip_suffix('M') {
        mb.parse::<u64>().unwrap_or(0) * 1024 * 1024
    } else {
        s.parse::<u64>().unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- detect_architecture ---

    #[test]
    fn detect_architecture_returns_known_arch() {
        let arch = detect_architecture();
        // On the build platform, the arch should be one of the known variants
        match std::env::consts::ARCH {
            "x86_64" => assert_eq!(arch, CpuArchitecture::X86_64),
            "aarch64" => assert_eq!(arch, CpuArchitecture::Aarch64),
            _ => assert_eq!(arch, CpuArchitecture::Unknown),
        }
    }

    // --- parse_cpuinfo with x86_64 format ---

    #[test]
    fn parse_cpuinfo_x86_64() {
        let content = "\
processor\t: 0
vendor_id\t: GenuineIntel
cpu family\t: 6
model\t\t: 143
model name\t: Intel(R) Xeon(R) w9-3495X
stepping\t: 8
cpu MHz\t\t: 1900.000
physical id\t: 0
cpu cores\t: 56
processor\t: 1
vendor_id\t: GenuineIntel
model name\t: Intel(R) Xeon(R) w9-3495X
physical id\t: 0
cpu cores\t: 56
flags\t\t: fpu vme de avx avx2 avx512f avx512bw
processor\t: 2
physical id\t: 1
cpu cores\t: 56
";
        let (model, features, physical, logical) = parse_cpuinfo(content);
        assert_eq!(model, "Intel(R) Xeon(R) w9-3495X");
        assert!(features.contains(&"avx512f".to_string()));
        assert!(features.contains(&"avx2".to_string()));
        assert_eq!(logical, 3);
        // 2 physical sockets * 56 cores = 112
        assert_eq!(physical, 112);
    }

    // --- parse_cpuinfo with aarch64 format ---

    #[test]
    fn parse_cpuinfo_aarch64() {
        let content = "\
processor\t: 0
BogoMIPS\t: 2000.00
CPU implementer\t: 0x41
CPU architecture: 8
CPU variant\t: 0x1
CPU part\t: 0xd40
CPU revision\t: 2
processor\t: 1
BogoMIPS\t: 2000.00
Features\t: fp asimd evtstrm aes pmull sha1 sha2 sve
";
        let (model, features, physical, logical) = parse_cpuinfo(content);
        assert!(model.contains("0x41"));
        assert!(model.contains("0xd40"));
        assert!(features.contains(&"sve".to_string()));
        assert!(features.contains(&"aes".to_string()));
        assert_eq!(logical, 2);
        // aarch64 with no physical id: physical == logical
        assert_eq!(physical, 2);
    }

    // --- parse_cpuinfo with empty input ---

    #[test]
    fn parse_cpuinfo_empty() {
        let (model, features, physical, logical) = parse_cpuinfo("");
        assert!(model.is_empty());
        assert!(features.is_empty());
        assert_eq!(physical, 0);
        assert_eq!(logical, 0);
    }

    // --- parse_cpuinfo with garbage ---

    #[test]
    fn parse_cpuinfo_garbage() {
        let (model, features, physical, logical) = parse_cpuinfo("random garbage\nno colons here");
        assert!(model.is_empty());
        assert!(features.is_empty());
        assert_eq!(physical, 0);
        assert_eq!(logical, 0);
    }

    // --- parse_cache_size ---

    #[test]
    fn parse_cache_size_kb() {
        assert_eq!(parse_cache_size("32768K"), 32768 * 1024);
    }

    #[test]
    fn parse_cache_size_mb() {
        assert_eq!(parse_cache_size("32M"), 32 * 1024 * 1024);
    }

    #[test]
    fn parse_cache_size_bytes() {
        assert_eq!(parse_cache_size("33554432"), 33_554_432);
    }

    #[test]
    fn parse_cache_size_empty() {
        assert_eq!(parse_cache_size(""), 0);
    }

    #[test]
    fn parse_cache_size_malformed() {
        assert_eq!(parse_cache_size("notanumber"), 0);
        assert_eq!(parse_cache_size("K"), 0);
    }

    // --- MockCpuBackend ---

    #[tokio::test]
    async fn mock_cpu_backend_returns_configured() {
        let cpu = CpuCapability {
            architecture: CpuArchitecture::X86_64,
            model: "Test CPU".into(),
            physical_cores: 16,
            logical_cores: 32,
            base_frequency_mhz: 2400,
            max_frequency_mhz: 4800,
            features: vec!["avx512f".into()],
            numa_nodes: 2,
            cache_l3_bytes: 32 * 1024 * 1024,
        };
        let backend = MockCpuBackend::with_cpu(cpu.clone());
        let detected = backend.detect().await.unwrap();
        assert_eq!(detected.model, "Test CPU");
        assert_eq!(detected.physical_cores, 16);
        assert_eq!(detected.logical_cores, 32);
    }

    #[tokio::test]
    async fn mock_cpu_backend_default() {
        let backend = MockCpuBackend::new();
        let detected = backend.detect().await.unwrap();
        assert_eq!(detected.architecture, CpuArchitecture::Unknown);
        assert!(detected.model.is_empty());
        assert_eq!(detected.physical_cores, 0);
    }
}
