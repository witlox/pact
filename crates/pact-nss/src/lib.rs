//! `pact-nss` — NSS module for OIDC→POSIX identity mapping.
//!
//! Produces `libnss_pact.so.2` which glibc loads when `nsswitch.conf`
//! includes `pact` for passwd and group databases:
//!
//! ```text
//! passwd: files pact
//! group:  files pact
//! ```
//!
//! The module reads from `/run/pact/passwd.db` and `/run/pact/group.db`
//! (JSON files written by pact-agent). It caches entries in memory and
//! re-reads on file modification.
//!
//! # Invariants
//!
//! - IM5: Read-only. Never writes, never makes network calls.
//! - IM6: Only active when pact is init (files exist only in PactSupervisor mode).
//!
//! # Licensing
//!
//! LGPL-3.0 — required because `libnss` crate is LGPL-3.0.
//! Dynamic linking (cdylib) satisfies LGPL requirements.

// Only compile on Linux (NSS is a glibc concept)
#![cfg(target_os = "linux")]

use std::sync::Mutex;

use libnss::group::{Group, GroupHooks};
use libnss::passwd::{Passwd, PasswdHooks};
use serde::{Deserialize, Serialize};

/// Well-known paths matching pact-agent's identity writer.
const PASSWD_DB_PATH: &str = "/run/pact/passwd.db";
const GROUP_DB_PATH: &str = "/run/pact/group.db";

// Register NSS hooks — this makes glibc call our functions.
libnss::libnss_passwd_hooks!(pact, PactPasswd);
libnss::libnss_group_hooks!(pact, PactGroup);

/// Cached passwd entry from passwd.db.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PasswdEntry {
    subject: String,
    uid: u32,
    gid: u32,
    username: String,
    home: String,
    shell: String,
    org: String,
}

/// Cached group entry from group.db.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GroupDbEntry {
    name: String,
    gid: u32,
    members: Vec<String>,
}

/// Global cache for passwd entries.
static PASSWD_CACHE: Mutex<Option<Vec<PasswdEntry>>> = Mutex::new(None);
/// Global cache for group entries.
static GROUP_CACHE: Mutex<Option<Vec<GroupDbEntry>>> = Mutex::new(None);

/// Load passwd entries from the .db file.
fn load_passwd() -> Vec<PasswdEntry> {
    match std::fs::read_to_string(PASSWD_DB_PATH) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// Load group entries from the .db file.
fn load_groups() -> Vec<GroupDbEntry> {
    match std::fs::read_to_string(GROUP_DB_PATH) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// Get or reload passwd cache.
fn get_passwd_entries() -> Vec<PasswdEntry> {
    let mut cache = PASSWD_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if cache.is_none() {
        *cache = Some(load_passwd());
    }
    cache.as_ref().cloned().unwrap_or_default()
}

/// Get or reload group cache.
fn get_group_entries() -> Vec<GroupDbEntry> {
    let mut cache = GROUP_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if cache.is_none() {
        *cache = Some(load_groups());
    }
    cache.as_ref().cloned().unwrap_or_default()
}

/// Convert a PasswdEntry to libnss Passwd.
fn to_passwd(e: &PasswdEntry) -> Passwd {
    Passwd {
        name: e.username.clone(),
        passwd: "x".to_string(),
        uid: e.uid,
        gid: e.gid,
        gecos: e.subject.clone(),
        dir: e.home.clone(),
        shell: e.shell.clone(),
    }
}

/// Convert a GroupDbEntry to libnss Group.
fn to_group(g: &GroupDbEntry) -> Group {
    Group {
        name: g.name.clone(),
        passwd: "x".to_string(),
        gid: g.gid,
        members: g.members.clone(),
    }
}

struct PactPasswd;

impl PasswdHooks for PactPasswd {
    fn get_all_entries() -> Vec<Passwd> {
        get_passwd_entries().iter().map(to_passwd).collect()
    }

    fn get_entry_by_uid(uid: libc::uid_t) -> Option<Passwd> {
        get_passwd_entries()
            .iter()
            .find(|e| e.uid == uid)
            .map(to_passwd)
    }

    fn get_entry_by_name(name: String) -> Option<Passwd> {
        get_passwd_entries()
            .iter()
            .find(|e| e.username == name)
            .map(to_passwd)
    }
}

struct PactGroup;

impl GroupHooks for PactGroup {
    fn get_all_entries() -> Vec<Group> {
        get_group_entries().iter().map(to_group).collect()
    }

    fn get_entry_by_gid(gid: libc::gid_t) -> Option<Group> {
        get_group_entries()
            .iter()
            .find(|g| g.gid == gid)
            .map(to_group)
    }

    fn get_entry_by_name(name: String) -> Option<Group> {
        get_group_entries()
            .iter()
            .find(|g| g.name == name)
            .map(to_group)
    }
}
