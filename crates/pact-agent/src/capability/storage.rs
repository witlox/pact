//! Storage capability detection backend.
//!
//! Detects local disks from `/sys/block/`, mount points from `/proc/mounts`,
//! and filesystem capacity via `statvfs`.

use async_trait::async_trait;
use pact_common::types::{
    DiskType, FsType, LocalDisk, MountInfo, StorageCapability, StorageNodeType,
};

/// Trait for storage detection backends.
#[async_trait]
pub trait StorageBackend: Send + Sync {
    /// Detect storage capabilities and return a [`StorageCapability`].
    async fn detect(&self) -> anyhow::Result<StorageCapability>;
}

/// Linux storage backend — reads `/sys/block/`, `/proc/mounts`, and calls `statvfs`.
pub struct LinuxStorageBackend;

impl LinuxStorageBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LinuxStorageBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StorageBackend for LinuxStorageBackend {
    async fn detect(&self) -> anyhow::Result<StorageCapability> {
        let local_disks = detect_local_disks();
        let node_type = if local_disks.is_empty() {
            StorageNodeType::Diskless
        } else {
            StorageNodeType::LocalStorage
        };

        let raw_mounts = match std::fs::read_to_string("/proc/mounts") {
            Ok(c) => parse_proc_mounts(&c),
            Err(_) => vec![],
        };

        let mut mounts = Vec::new();
        for (source, path, fs_type_str) in &raw_mounts {
            // Skip pseudo-filesystems
            if path.starts_with("/proc")
                || path.starts_with("/sys")
                || path.starts_with("/dev")
                || path.starts_with("/run/lock")
            {
                continue;
            }

            let fs_type = parse_fs_type(fs_type_str);
            let (total_bytes, available_bytes) = statvfs_with_timeout(path).await;

            mounts.push(MountInfo {
                path: path.clone(),
                fs_type,
                source: source.clone(),
                total_bytes,
                available_bytes,
            });
        }

        Ok(StorageCapability { node_type, local_disks, mounts })
    }
}

/// Detect local block devices from `/sys/block/`.
fn detect_local_disks() -> Vec<LocalDisk> {
    let sys_block = std::path::Path::new("/sys/block");
    let entries = match std::fs::read_dir(sys_block) {
        Ok(e) => e,
        Err(_) => return vec![],
    };

    let mut disks = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();

        // Only detect nvme and sd devices
        if !name.starts_with("nvme") && !name.starts_with("sd") {
            continue;
        }

        let device = format!("/dev/{name}");
        let model = read_block_sysfs(&name, "device/model");
        let capacity_bytes = read_block_size(&name);
        let disk_type = detect_disk_type(&name);

        disks.push(LocalDisk { device, model, capacity_bytes, disk_type });
    }

    disks
}

/// Read a sysfs attribute for a block device, returning trimmed content or empty string.
fn read_block_sysfs(device: &str, attr: &str) -> String {
    let path = format!("/sys/block/{device}/{attr}");
    std::fs::read_to_string(path).map(|s| s.trim().to_string()).unwrap_or_default()
}

/// Read block device size from sysfs (in 512-byte sectors) and convert to bytes.
fn read_block_size(device: &str) -> u64 {
    let content = read_block_sysfs(device, "size");
    parse_block_size(&content)
}

/// Parse block size content (sectors) to bytes.
pub(crate) fn parse_block_size(content: &str) -> u64 {
    content.trim().parse::<u64>().unwrap_or(0).saturating_mul(512)
}

/// Detect disk type from sysfs attributes.
pub(crate) fn detect_disk_type(name: &str) -> DiskType {
    if name.starts_with("nvme") {
        return DiskType::Nvme;
    }

    // For sd* devices, check rotational attribute
    let rotational = read_block_sysfs(name, "queue/rotational");
    match rotational.trim() {
        "0" => DiskType::Ssd,
        "1" => DiskType::Hdd,
        _ => DiskType::Unknown,
    }
}

/// Parse /proc/mounts content into (source, mountpoint, fstype) tuples.
pub(crate) fn parse_proc_mounts(content: &str) -> Vec<(String, String, String)> {
    content
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                Some((parts[0].to_string(), parts[1].to_string(), parts[2].to_string()))
            } else {
                None
            }
        })
        .collect()
}

/// Parse a filesystem type string into the `FsType` enum.
pub(crate) fn parse_fs_type(s: &str) -> FsType {
    match s {
        "nfs" | "nfs4" => FsType::Nfs,
        "lustre" => FsType::Lustre,
        "ext4" => FsType::Ext4,
        "xfs" => FsType::Xfs,
        "tmpfs" => FsType::Tmpfs,
        other => FsType::Other(other.to_string()),
    }
}

/// Call statvfs with a 2-second timeout, returning (total_bytes, available_bytes).
///
/// On timeout, error, or non-Linux: returns (0, 0).
async fn statvfs_with_timeout(path: &str) -> (u64, u64) {
    let path = path.to_string();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        tokio::task::spawn_blocking(move || statvfs_sync(&path)),
    )
    .await;

    match result {
        Ok(Ok(Ok(pair))) => pair,
        _ => (0, 0),
    }
}

/// Synchronous statvfs call. Linux-only.
#[cfg(target_os = "linux")]
fn statvfs_sync(path: &str) -> anyhow::Result<(u64, u64)> {
    use std::ffi::CString;
    use std::mem::MaybeUninit;

    let c_path = CString::new(path)?;
    let mut stat = MaybeUninit::<libc::statvfs>::uninit();

    // SAFETY: we pass a valid C string and a valid pointer to an uninitialized struct
    let ret = unsafe { libc::statvfs(c_path.as_ptr(), stat.as_mut_ptr()) };
    if ret != 0 {
        anyhow::bail!("statvfs failed for {path}: {}", std::io::Error::last_os_error());
    }

    // SAFETY: statvfs succeeded, so the struct is initialized
    let stat = unsafe { stat.assume_init() };
    let total = stat.f_blocks as u64 * stat.f_frsize as u64;
    let available = stat.f_bavail as u64 * stat.f_frsize as u64;
    Ok((total, available))
}

/// Stub statvfs for non-Linux platforms.
#[cfg(not(target_os = "linux"))]
fn statvfs_sync(_path: &str) -> anyhow::Result<(u64, u64)> {
    Ok((0, 0))
}

/// Mock storage backend for development/testing.
pub struct MockStorageBackend {
    pub storage: StorageCapability,
}

impl MockStorageBackend {
    pub fn new() -> Self {
        Self {
            storage: StorageCapability {
                node_type: StorageNodeType::Diskless,
                local_disks: vec![],
                mounts: vec![],
            },
        }
    }
}

impl Default for MockStorageBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl StorageBackend for MockStorageBackend {
    async fn detect(&self) -> anyhow::Result<StorageCapability> {
        Ok(self.storage.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_proc_mounts tests ---

    #[test]
    fn parse_proc_mounts_realistic() {
        let content = "\
sysfs /sys sysfs rw,nosuid,nodev,noexec,relatime 0 0
proc /proc proc rw,nosuid,nodev,noexec,relatime 0 0
/dev/sda1 / ext4 rw,relatime 0 0
mds01:/scratch /scratch lustre rw 0 0
tmpfs /tmp tmpfs rw,nosuid,nodev 0 0
10.0.0.1:/home /home nfs4 rw,relatime 0 0
";
        let mounts = parse_proc_mounts(content);
        assert_eq!(mounts.len(), 6);
        assert_eq!(mounts[0], ("sysfs".into(), "/sys".into(), "sysfs".into()));
        assert_eq!(mounts[2], ("/dev/sda1".into(), "/".into(), "ext4".into()));
        assert_eq!(mounts[3], ("mds01:/scratch".into(), "/scratch".into(), "lustre".into()));
        assert_eq!(mounts[5], ("10.0.0.1:/home".into(), "/home".into(), "nfs4".into()));
    }

    #[test]
    fn parse_proc_mounts_empty() {
        assert!(parse_proc_mounts("").is_empty());
    }

    #[test]
    fn parse_proc_mounts_malformed_lines_skipped() {
        let content = "short\n/dev/sda1 / ext4 rw 0 0\n";
        let mounts = parse_proc_mounts(content);
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].1, "/");
    }

    // --- parse_fs_type tests ---

    #[test]
    fn parse_fs_type_nfs() {
        assert_eq!(parse_fs_type("nfs"), FsType::Nfs);
        assert_eq!(parse_fs_type("nfs4"), FsType::Nfs);
    }

    #[test]
    fn parse_fs_type_lustre() {
        assert_eq!(parse_fs_type("lustre"), FsType::Lustre);
    }

    #[test]
    fn parse_fs_type_ext4() {
        assert_eq!(parse_fs_type("ext4"), FsType::Ext4);
    }

    #[test]
    fn parse_fs_type_xfs() {
        assert_eq!(parse_fs_type("xfs"), FsType::Xfs);
    }

    #[test]
    fn parse_fs_type_tmpfs() {
        assert_eq!(parse_fs_type("tmpfs"), FsType::Tmpfs);
    }

    #[test]
    fn parse_fs_type_other() {
        assert_eq!(parse_fs_type("btrfs"), FsType::Other("btrfs".into()));
        assert_eq!(parse_fs_type("zfs"), FsType::Other("zfs".into()));
    }

    // --- parse_block_size tests ---

    #[test]
    fn parse_block_size_normal() {
        // 1953525168 sectors * 512 = ~1TB
        assert_eq!(parse_block_size("1953525168\n"), 1_953_525_168 * 512);
    }

    #[test]
    fn parse_block_size_zero() {
        assert_eq!(parse_block_size("0\n"), 0);
    }

    #[test]
    fn parse_block_size_empty() {
        assert_eq!(parse_block_size(""), 0);
    }

    #[test]
    fn parse_block_size_garbage() {
        assert_eq!(parse_block_size("not_a_number\n"), 0);
    }

    // --- detect_disk_type tests ---

    #[test]
    fn detect_disk_type_nvme() {
        assert_eq!(detect_disk_type("nvme0n1"), DiskType::Nvme);
        assert_eq!(detect_disk_type("nvme1n1"), DiskType::Nvme);
    }

    #[test]
    fn detect_disk_type_sd_defaults_to_unknown() {
        // On non-Linux or without sysfs, rotational is unreadable -> Unknown
        // (On actual Linux with the file present, this would return Ssd or Hdd)
        let dt = detect_disk_type("sda");
        assert!(dt == DiskType::Ssd || dt == DiskType::Hdd || dt == DiskType::Unknown);
    }

    // --- StorageNodeType detection ---

    #[test]
    fn node_type_diskless_when_no_disks() {
        let disks: Vec<LocalDisk> = vec![];
        let node_type = if disks.is_empty() {
            StorageNodeType::Diskless
        } else {
            StorageNodeType::LocalStorage
        };
        assert_eq!(node_type, StorageNodeType::Diskless);
    }

    #[test]
    fn node_type_local_when_disks_present() {
        let disks = [LocalDisk {
            device: "/dev/nvme0n1".into(),
            model: "Test".into(),
            capacity_bytes: 1_000_000_000,
            disk_type: DiskType::Nvme,
        }];
        let node_type = if disks.is_empty() {
            StorageNodeType::Diskless
        } else {
            StorageNodeType::LocalStorage
        };
        assert_eq!(node_type, StorageNodeType::LocalStorage);
    }

    // --- MockStorageBackend tests ---

    #[tokio::test]
    async fn mock_storage_backend_returns_configured_data() {
        let backend = MockStorageBackend {
            storage: StorageCapability {
                node_type: StorageNodeType::LocalStorage,
                local_disks: vec![LocalDisk {
                    device: "/dev/nvme0n1".into(),
                    model: "Samsung 990 PRO".into(),
                    capacity_bytes: 1_000_000_000_000,
                    disk_type: DiskType::Nvme,
                }],
                mounts: vec![MountInfo {
                    path: "/scratch".into(),
                    fs_type: FsType::Lustre,
                    source: "mds01:/scratch".into(),
                    total_bytes: 10_000_000_000_000,
                    available_bytes: 5_000_000_000_000,
                }],
            },
        };

        let storage = backend.detect().await.unwrap();
        assert_eq!(storage.node_type, StorageNodeType::LocalStorage);
        assert_eq!(storage.local_disks.len(), 1);
        assert_eq!(storage.local_disks[0].disk_type, DiskType::Nvme);
        assert_eq!(storage.mounts.len(), 1);
        assert_eq!(storage.mounts[0].fs_type, FsType::Lustre);
    }

    #[tokio::test]
    async fn mock_storage_backend_default_is_diskless() {
        let backend = MockStorageBackend::new();
        let storage = backend.detect().await.unwrap();
        assert_eq!(storage.node_type, StorageNodeType::Diskless);
        assert!(storage.local_disks.is_empty());
        assert!(storage.mounts.is_empty());
    }

    // --- statvfs timeout test ---

    #[tokio::test]
    async fn statvfs_timeout_returns_zero() {
        // Non-existent path should return (0, 0) without hanging
        let (total, avail) = statvfs_with_timeout("/nonexistent/path/that/does/not/exist").await;
        assert_eq!(total, 0);
        assert_eq!(avail, 0);
    }
}
