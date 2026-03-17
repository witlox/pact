//! Identity mapping — UidMap cache and NSS .db file writer.
//!
//! Manages the local OIDC → POSIX mapping for NFS compatibility.
//! Only active in PactSupervisor mode (IM6).
//!
//! The writer produces simple JSON files at `/run/pact/passwd.db`
//! and `/run/pact/group.db` that the NSS module (`libnss_pact.so`)
//! reads via mmap.
//!
//! # File format
//!
//! passwd.db: JSON array of UidEntry objects
//! group.db: JSON array of GroupEntry objects
//!
//! Using JSON for simplicity and debuggability. The NSS module
//! deserializes on load and caches in memory. For ~10,000 entries
//! this is <1MB and parses in <1ms.

use std::path::{Path, PathBuf};

use pact_common::types::{GroupEntry, UidEntry, UidMap};
use tracing::{debug, info, warn};

/// Well-known paths for NSS .db files.
pub const PASSWD_DB_PATH: &str = "/run/pact/passwd.db";
pub const GROUP_DB_PATH: &str = "/run/pact/group.db";

/// Identity mapping manager — writes .db files from UidMap.
pub struct IdentityManager {
    /// Base directory for .db files (default: /run/pact).
    base_dir: PathBuf,
    /// Cached UidMap from journal.
    uid_map: UidMap,
    /// Whether identity mapping is active.
    active: bool,
}

impl IdentityManager {
    /// Create a new identity manager.
    ///
    /// `active` should be true only when `SupervisorBackend::Pact` and NFS is used.
    #[must_use]
    pub fn new(base_dir: &str, active: bool) -> Self {
        Self {
            base_dir: PathBuf::from(base_dir),
            uid_map: UidMap::new(),
            active,
        }
    }

    /// Update the cached UidMap (from journal subscription).
    pub fn update_map(&mut self, uid_map: UidMap) {
        self.uid_map = uid_map;
        if self.active {
            if let Err(e) = self.write_db_files() {
                warn!("failed to write identity .db files: {e}");
            }
        }
    }

    /// Get a reference to the current UidMap.
    #[must_use]
    pub fn uid_map(&self) -> &UidMap {
        &self.uid_map
    }

    /// Whether identity mapping is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Write passwd.db and group.db files.
    ///
    /// Called on boot (Phase 3: LoadIdentity) and on UidMap updates.
    pub fn write_db_files(&self) -> anyhow::Result<()> {
        if !self.active {
            debug!("identity mapping inactive, skipping .db file write");
            return Ok(());
        }

        // Ensure directory exists
        std::fs::create_dir_all(&self.base_dir)?;

        // Write passwd.db
        let passwd_path = self.base_dir.join("passwd.db");
        let users: Vec<&UidEntry> = self.uid_map.users.values().collect();
        let passwd_json = serde_json::to_string_pretty(&users)?;
        write_atomic(&passwd_path, passwd_json.as_bytes())?;
        info!(
            path = %passwd_path.display(),
            entries = users.len(),
            "wrote passwd.db"
        );

        // Write group.db
        let group_path = self.base_dir.join("group.db");
        let groups: Vec<&GroupEntry> = self.uid_map.groups.values().collect();
        let group_json = serde_json::to_string_pretty(&groups)?;
        write_atomic(&group_path, group_json.as_bytes())?;
        info!(
            path = %group_path.display(),
            entries = groups.len(),
            "wrote group.db"
        );

        Ok(())
    }

    /// Look up a user by UID (from cached map).
    #[must_use]
    pub fn get_by_uid(&self, uid: u32) -> Option<&UidEntry> {
        self.uid_map.get_by_uid(uid)
    }

    /// Look up a user by username (from cached map).
    #[must_use]
    pub fn get_by_username(&self, username: &str) -> Option<&UidEntry> {
        self.uid_map.get_by_username(username)
    }
}

/// Write a file atomically (write to tmp, rename).
///
/// Prevents partial reads by the NSS module.
fn write_atomic(path: &Path, data: &[u8]) -> anyhow::Result<()> {
    let tmp_path = path.with_extension("tmp");
    std::fs::write(&tmp_path, data)?;

    // Set permissions to 0644 (world-readable, needed for NSS)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o644))?;
    }

    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

/// Read a passwd.db file back into a Vec<UidEntry>.
///
/// Used by the NSS module and for testing.
pub fn read_passwd_db(path: &Path) -> anyhow::Result<Vec<UidEntry>> {
    let data = std::fs::read_to_string(path)?;
    let entries: Vec<UidEntry> = serde_json::from_str(&data)?;
    Ok(entries)
}

/// Read a group.db file back into a Vec<GroupEntry>.
pub fn read_group_db(path: &Path) -> anyhow::Result<Vec<GroupEntry>> {
    let data = std::fs::read_to_string(path)?;
    let entries: Vec<GroupEntry> = serde_json::from_str(&data)?;
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pact_common::types::OrgIndex;
    use tempfile::TempDir;

    fn test_uid_map() -> UidMap {
        let mut map = UidMap::new();
        map.org_indices.push(OrgIndex {
            org: "local".into(),
            index: 0,
        });
        map.assign_uid("user1@cscs.ch", "pwitlox", "local", "/users/pwitlox", "/bin/bash")
            .unwrap();
        map.assign_uid("user2@cscs.ch", "jdoe", "local", "/users/jdoe", "/bin/bash")
            .unwrap();
        map.groups.insert(
            "lp16".into(),
            GroupEntry {
                name: "lp16".into(),
                gid: 2001,
                members: vec!["pwitlox".into(), "jdoe".into()],
            },
        );
        map
    }

    #[test]
    fn write_and_read_passwd_db() {
        let dir = TempDir::new().unwrap();
        let mgr = IdentityManager::new(dir.path().to_str().unwrap(), true);

        let mut mgr = mgr;
        mgr.update_map(test_uid_map());

        let passwd_path = dir.path().join("passwd.db");
        let entries = read_passwd_db(&passwd_path).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().any(|e| e.username == "pwitlox"));
        assert!(entries.iter().any(|e| e.username == "jdoe"));
    }

    #[test]
    fn write_and_read_group_db() {
        let dir = TempDir::new().unwrap();
        let mut mgr = IdentityManager::new(dir.path().to_str().unwrap(), true);
        mgr.update_map(test_uid_map());

        let group_path = dir.path().join("group.db");
        let groups = read_group_db(&group_path).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "lp16");
        assert_eq!(groups[0].members.len(), 2);
    }

    #[test]
    fn inactive_manager_skips_write() {
        let dir = TempDir::new().unwrap();
        let mut mgr = IdentityManager::new(dir.path().to_str().unwrap(), false);
        mgr.update_map(test_uid_map());

        // Files should not be created
        assert!(!dir.path().join("passwd.db").exists());
        assert!(!dir.path().join("group.db").exists());
    }

    #[test]
    fn atomic_write_no_partial_reads() {
        let dir = TempDir::new().unwrap();
        let mut mgr = IdentityManager::new(dir.path().to_str().unwrap(), true);

        // Write first version
        let mut map1 = test_uid_map();
        mgr.update_map(map1.clone());

        // Write second version (should atomically replace)
        map1.assign_uid("user3@cscs.ch", "newuser", "local", "/users/newuser", "/bin/bash")
            .unwrap();
        mgr.update_map(map1);

        let entries = read_passwd_db(&dir.path().join("passwd.db")).unwrap();
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn lookup_from_cached_map() {
        let mut mgr = IdentityManager::new("/tmp/test", true);
        mgr.uid_map = test_uid_map();

        let entry = mgr.get_by_username("pwitlox").unwrap();
        assert_eq!(entry.uid, 10_000);

        let entry = mgr.get_by_uid(10_001).unwrap();
        assert_eq!(entry.username, "jdoe");
    }

    #[test]
    fn empty_map_writes_empty_arrays() {
        let dir = TempDir::new().unwrap();
        let mut mgr = IdentityManager::new(dir.path().to_str().unwrap(), true);
        mgr.update_map(UidMap::new());

        let entries = read_passwd_db(&dir.path().join("passwd.db")).unwrap();
        assert!(entries.is_empty());

        let groups = read_group_db(&dir.path().join("group.db")).unwrap();
        assert!(groups.is_empty());
    }
}
