//! Memory capability detection backend.
//!
//! Detects system memory, NUMA topology, huge pages, and memory type
//! from `/proc/meminfo`, sysfs, and optionally `dmidecode` on Linux.

use async_trait::async_trait;
use pact_common::types::{HugePageInfo, MemoryCapability, MemoryType, NumaNode};

/// Trait for memory detection backends.
#[async_trait]
pub trait MemoryBackend: Send + Sync {
    /// Detect memory capabilities and return a [`MemoryCapability`].
    async fn detect(&self) -> anyhow::Result<MemoryCapability>;
}

/// Linux memory backend — reads `/proc/meminfo`, sysfs, and dmidecode.
pub struct LinuxMemoryBackend;

impl LinuxMemoryBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LinuxMemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MemoryBackend for LinuxMemoryBackend {
    async fn detect(&self) -> anyhow::Result<MemoryCapability> {
        let meminfo = tokio::fs::read_to_string("/proc/meminfo").await.unwrap_or_default();

        let (total_bytes, available_bytes, small_hp) = parse_meminfo(&meminfo);
        let giant_hp = detect_1gb_hugepages().await;
        let hugepages = HugePageInfo {
            size_2mb_total: small_hp.0,
            size_2mb_free: small_hp.1,
            size_1gb_total: giant_hp.0,
            size_1gb_free: giant_hp.1,
        };

        let (numa_count, numa_topology) = detect_numa_topology().await;
        let memory_type = detect_memory_type().await;

        Ok(MemoryCapability {
            total_bytes,
            available_bytes,
            memory_type,
            numa_nodes: numa_count,
            numa_topology,
            hugepages,
        })
    }
}

/// Mock memory backend for development/testing.
pub struct MockMemoryBackend {
    pub memory: MemoryCapability,
}

impl MockMemoryBackend {
    pub fn new() -> Self {
        Self {
            memory: MemoryCapability {
                total_bytes: 0,
                available_bytes: 0,
                memory_type: MemoryType::default(),
                numa_nodes: 1,
                numa_topology: vec![],
                hugepages: HugePageInfo::default(),
            },
        }
    }

    /// Create a mock with pre-configured memory capability.
    pub fn with_memory(memory: MemoryCapability) -> Self {
        Self { memory }
    }
}

impl Default for MockMemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MemoryBackend for MockMemoryBackend {
    async fn detect(&self) -> anyhow::Result<MemoryCapability> {
        Ok(self.memory.clone())
    }
}

/// Parse `/proc/meminfo` content.
///
/// Returns `(total_bytes, available_bytes, (hugepages_2mb_total, hugepages_2mb_free))`.
fn parse_meminfo(content: &str) -> (u64, u64, (u64, u64)) {
    let mut total = 0u64;
    let mut available = 0u64;
    let mut hp_total = 0u64;
    let mut hp_free = 0u64;
    let mut hp_size_kb = 0u64;

    for line in content.lines() {
        if let Some(kb) = parse_meminfo_kb(line) {
            if line.starts_with("MemTotal:") {
                total = kb * 1024;
            } else if line.starts_with("MemAvailable:") {
                available = kb * 1024;
            } else if line.starts_with("HugePages_Total:") {
                hp_total = kb; // This is a count, not kB
            } else if line.starts_with("HugePages_Free:") {
                hp_free = kb; // This is a count, not kB
            } else if line.starts_with("Hugepagesize:") {
                hp_size_kb = kb;
            }
        }
    }

    // HugePages_Total/Free from /proc/meminfo are for the default hugepage size.
    // If the default hugepage size is 2048 kB (2MB), use those counts.
    let (hp_2mb_total, hp_2mb_free) = if hp_size_kb == 2048 { (hp_total, hp_free) } else { (0, 0) };

    (total, available, (hp_2mb_total, hp_2mb_free))
}

/// Parse a single line from `/proc/meminfo`, extracting the numeric value.
///
/// Works for both "kB" lines (MemTotal, MemAvailable, Hugepagesize) and
/// pure count lines (HugePages_Total, HugePages_Free).
fn parse_meminfo_kb(line: &str) -> Option<u64> {
    line.split_whitespace().nth(1)?.parse().ok()
}

/// Detect 1GB huge pages from sysfs.
async fn detect_1gb_hugepages() -> (u64, u64) {
    let base = "/sys/kernel/mm/hugepages/hugepages-1048576kB";
    let total = read_sysfs_u64(&format!("{base}/nr_hugepages")).await.unwrap_or(0);
    let free = read_sysfs_u64(&format!("{base}/free_hugepages")).await.unwrap_or(0);
    (total, free)
}

/// Read a sysfs file containing a single u64 value.
async fn read_sysfs_u64(path: &str) -> Option<u64> {
    let content = tokio::fs::read_to_string(path).await.ok()?;
    content.trim().parse::<u64>().ok()
}

/// Detect NUMA topology from sysfs.
///
/// Returns `(node_count, Vec<NumaNode>)`.
async fn detect_numa_topology() -> (u32, Vec<NumaNode>) {
    let node_path = std::path::Path::new("/sys/devices/system/node");
    if !node_path.exists() {
        return (1, vec![]);
    }

    let mut nodes = Vec::new();
    if let Ok(mut entries) = tokio::fs::read_dir(node_path).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if let Some(name) = entry.file_name().to_str() {
                if let Some(id_str) = name.strip_prefix("node") {
                    if let Ok(id) = id_str.parse::<u32>() {
                        let node_dir = entry.path();

                        // Read per-node memory from meminfo
                        let total_bytes =
                            read_node_memtotal(&node_dir.join("meminfo")).await.unwrap_or(0);

                        // Read per-node CPU list
                        let cpus = if let Ok(cpulist) =
                            tokio::fs::read_to_string(node_dir.join("cpulist")).await
                        {
                            parse_cpulist(cpulist.trim())
                        } else {
                            vec![]
                        };

                        nodes.push(NumaNode { id, total_bytes, cpus });
                    }
                }
            }
        }
    }

    nodes.sort_by_key(|n| n.id);
    let count = if nodes.is_empty() { 1 } else { nodes.len() as u32 };
    (count, nodes)
}

/// Read MemTotal from a per-NUMA-node meminfo file.
///
/// Format: `Node 0 MemTotal:       447983424 kB`
async fn read_node_memtotal(path: &std::path::Path) -> Option<u64> {
    let content = tokio::fs::read_to_string(path).await.ok()?;
    for line in content.lines() {
        if line.contains("MemTotal:") {
            // Parse the kB value
            let parts: Vec<&str> = line.split_whitespace().collect();
            // Find the value right before "kB"
            for (i, part) in parts.iter().enumerate() {
                if *part == "kB" && i > 0 {
                    if let Ok(kb) = parts[i - 1].parse::<u64>() {
                        return Some(kb * 1024);
                    }
                }
            }
        }
    }
    None
}

/// Parse a CPU list string like "0-27,112-139" into individual CPU IDs.
///
/// Handles:
/// - Single values: "5" -> [5]
/// - Ranges: "0-3" -> [0, 1, 2, 3]
/// - Mixed: "0-3,8,12-15" -> [0, 1, 2, 3, 8, 12, 13, 14, 15]
/// - Empty: "" -> []
pub(crate) fn parse_cpulist(content: &str) -> Vec<u32> {
    let content = content.trim();
    if content.is_empty() {
        return vec![];
    }

    let mut cpus = Vec::new();
    for part in content.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((start, end)) = part.split_once('-') {
            if let (Ok(s), Ok(e)) = (start.trim().parse::<u32>(), end.trim().parse::<u32>()) {
                cpus.extend(s..=e);
            }
        } else if let Ok(v) = part.parse::<u32>() {
            cpus.push(v);
        }
    }
    cpus
}

/// Detect memory type using `dmidecode --type 17` with a 2-second timeout.
///
/// Falls back to `Unknown` on timeout, failure, or insufficient permissions.
async fn detect_memory_type() -> MemoryType {
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        tokio::process::Command::new("dmidecode").args(["--type", "17"]).output(),
    )
    .await;

    match result {
        Ok(Ok(output)) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            parse_dmidecode_memory_type(&stdout)
        }
        _ => MemoryType::Unknown,
    }
}

/// Parse memory type from `dmidecode --type 17` output.
///
/// Looks for lines like "Type: DDR5" or "Type: HBM2e".
fn parse_dmidecode_memory_type(output: &str) -> MemoryType {
    for line in output.lines() {
        let line = line.trim();
        if let Some(type_val) = line.strip_prefix("Type:") {
            let type_val = type_val.trim();
            return match type_val {
                "DDR4" => MemoryType::Ddr4,
                "DDR5" => MemoryType::Ddr5,
                s if s.eq_ignore_ascii_case("HBM2E") || s.eq_ignore_ascii_case("HBM2e") => {
                    MemoryType::Hbm2e
                }
                s if s.eq_ignore_ascii_case("HBM3") => MemoryType::Hbm3,
                s if s.eq_ignore_ascii_case("HBM3E") || s.eq_ignore_ascii_case("HBM3e") => {
                    MemoryType::Hbm3e
                }
                _ => MemoryType::Unknown,
            };
        }
    }
    MemoryType::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_meminfo ---

    #[test]
    fn parse_meminfo_real_gh200_format() {
        let content = "\
MemTotal:       895966848 kB
MemFree:        670225408 kB
MemAvailable:   800000000 kB
Buffers:          234567 kB
Cached:         20000000 kB
HugePages_Total:     128
HugePages_Free:       64
Hugepagesize:       2048 kB
";
        let (total, avail, (hp_total, hp_free)) = parse_meminfo(content);
        assert_eq!(total, 895_966_848 * 1024);
        assert_eq!(avail, 800_000_000 * 1024);
        assert_eq!(hp_total, 128);
        assert_eq!(hp_free, 64);
    }

    #[test]
    fn parse_meminfo_missing_fields() {
        let content = "MemTotal:       16000000 kB\nMemFree:        8000000 kB\n";
        let (total, avail, (hp_total, hp_free)) = parse_meminfo(content);
        assert_eq!(total, 16_000_000 * 1024);
        assert_eq!(avail, 0); // MemAvailable not present
        assert_eq!(hp_total, 0);
        assert_eq!(hp_free, 0);
    }

    #[test]
    fn parse_meminfo_empty() {
        let (total, avail, (hp_total, hp_free)) = parse_meminfo("");
        assert_eq!(total, 0);
        assert_eq!(avail, 0);
        assert_eq!(hp_total, 0);
        assert_eq!(hp_free, 0);
    }

    #[test]
    fn parse_meminfo_garbage() {
        let (total, avail, _) = parse_meminfo("this is not meminfo\nrandom garbage");
        assert_eq!(total, 0);
        assert_eq!(avail, 0);
    }

    #[test]
    fn parse_meminfo_non_2mb_hugepages() {
        // If default hugepage size is not 2MB, don't report as 2MB
        let content = "\
MemTotal:       65536000 kB
HugePages_Total:     16
HugePages_Free:       8
Hugepagesize:    1048576 kB
";
        let (_, _, (hp_total, hp_free)) = parse_meminfo(content);
        assert_eq!(hp_total, 0); // Not 2MB pages
        assert_eq!(hp_free, 0);
    }

    // --- parse_cpulist ---

    #[test]
    fn parse_cpulist_single_range() {
        let cpus = parse_cpulist("0-3");
        assert_eq!(cpus, vec![0, 1, 2, 3]);
    }

    #[test]
    fn parse_cpulist_multiple_ranges() {
        let cpus = parse_cpulist("0-27,112-139");
        assert_eq!(cpus.len(), 56); // 28 + 28
        assert_eq!(cpus[0], 0);
        assert_eq!(cpus[27], 27);
        assert_eq!(cpus[28], 112);
        assert_eq!(cpus[55], 139);
    }

    #[test]
    fn parse_cpulist_mixed() {
        let cpus = parse_cpulist("0-3,8,12-15");
        assert_eq!(cpus, vec![0, 1, 2, 3, 8, 12, 13, 14, 15]);
    }

    #[test]
    fn parse_cpulist_single_value() {
        let cpus = parse_cpulist("5");
        assert_eq!(cpus, vec![5]);
    }

    #[test]
    fn parse_cpulist_empty() {
        let cpus = parse_cpulist("");
        assert!(cpus.is_empty());
    }

    #[test]
    fn parse_cpulist_whitespace() {
        let cpus = parse_cpulist("  0-3 , 8 , 12-15  ");
        assert_eq!(cpus, vec![0, 1, 2, 3, 8, 12, 13, 14, 15]);
    }

    #[test]
    fn parse_cpulist_malformed_skipped() {
        let cpus = parse_cpulist("0-3,abc,8");
        assert_eq!(cpus, vec![0, 1, 2, 3, 8]);
    }

    // --- parse_dmidecode_memory_type ---

    #[test]
    fn parse_dmidecode_ddr5() {
        let output = "\
Memory Device
\tSize: 32 GB
\tType: DDR5
\tSpeed: 4800 MT/s
";
        assert_eq!(parse_dmidecode_memory_type(output), MemoryType::Ddr5);
    }

    #[test]
    fn parse_dmidecode_ddr4() {
        let output = "\tType: DDR4\n";
        assert_eq!(parse_dmidecode_memory_type(output), MemoryType::Ddr4);
    }

    #[test]
    fn parse_dmidecode_hbm2e() {
        let output = "\tType: HBM2e\n";
        assert_eq!(parse_dmidecode_memory_type(output), MemoryType::Hbm2e);
    }

    #[test]
    fn parse_dmidecode_hbm3() {
        let output = "\tType: HBM3\n";
        assert_eq!(parse_dmidecode_memory_type(output), MemoryType::Hbm3);
    }

    #[test]
    fn parse_dmidecode_hbm3e() {
        let output = "\tType: HBM3e\n";
        assert_eq!(parse_dmidecode_memory_type(output), MemoryType::Hbm3e);
    }

    #[test]
    fn parse_dmidecode_unknown_type() {
        let output = "\tType: LPDDR5\n";
        assert_eq!(parse_dmidecode_memory_type(output), MemoryType::Unknown);
    }

    #[test]
    fn parse_dmidecode_empty() {
        assert_eq!(parse_dmidecode_memory_type(""), MemoryType::Unknown);
    }

    #[test]
    fn parse_dmidecode_no_type_line() {
        let output = "Memory Device\n\tSize: 32 GB\n\tSpeed: 4800 MT/s\n";
        assert_eq!(parse_dmidecode_memory_type(output), MemoryType::Unknown);
    }

    // --- MockMemoryBackend ---

    #[tokio::test]
    async fn mock_memory_backend_returns_configured() {
        let mem = MemoryCapability {
            total_bytes: 1024 * 1024 * 1024,
            available_bytes: 512 * 1024 * 1024,
            memory_type: MemoryType::Ddr5,
            numa_nodes: 2,
            numa_topology: vec![
                NumaNode { id: 0, total_bytes: 512 * 1024 * 1024, cpus: vec![0, 1, 2, 3] },
                NumaNode { id: 1, total_bytes: 512 * 1024 * 1024, cpus: vec![4, 5, 6, 7] },
            ],
            hugepages: HugePageInfo::default(),
        };
        let backend = MockMemoryBackend::with_memory(mem);
        let detected = backend.detect().await.unwrap();
        assert_eq!(detected.total_bytes, 1024 * 1024 * 1024);
        assert_eq!(detected.memory_type, MemoryType::Ddr5);
        assert_eq!(detected.numa_topology.len(), 2);
    }

    #[tokio::test]
    async fn mock_memory_backend_default() {
        let backend = MockMemoryBackend::new();
        let detected = backend.detect().await.unwrap();
        assert_eq!(detected.total_bytes, 0);
        assert_eq!(detected.memory_type, MemoryType::Unknown);
        assert_eq!(detected.numa_nodes, 1);
    }
}
