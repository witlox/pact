//! E2E test: Real cgroup v2 operations on Linux.
//!
//! These tests require:
//! - Linux with cgroup v2 unified hierarchy
//! - Root or CAP_SYS_ADMIN for cgroup creation
//! - Run with `--test-threads=1` to avoid cgroup conflicts
//!
//! Skipped on non-Linux platforms and when not running as root.

#![cfg(target_os = "linux")]
#![allow(unused_imports)] // Some imports only used in specific test paths

use std::fs;
use std::path::Path;
use std::process::Command;

use hpc_node::cgroup::slices;
use hpc_node::{CgroupManager, ResourceLimits};
use pact_agent::isolation::LinuxCgroupManager;

/// Check if we can create cgroups (need root or CAP_SYS_ADMIN).
fn can_create_cgroups() -> bool {
    // Check if /sys/fs/cgroup is writable
    let test_path = "/sys/fs/cgroup/pact-e2e-test";
    match fs::create_dir(test_path) {
        Ok(()) => {
            let _ = fs::remove_dir(test_path);
            true
        }
        Err(_) => false,
    }
}

/// Check if cgroup v2 unified hierarchy is mounted.
fn has_cgroup_v2() -> bool {
    Path::new("/sys/fs/cgroup/cgroup.controllers").exists()
}

#[tokio::test]
#[ignore = "requires root/CAP_SYS_ADMIN on Linux"]
async fn cgroup_hierarchy_creation() {
    if !has_cgroup_v2() || !can_create_cgroups() {
        eprintln!("SKIP: requires cgroup v2 + root/CAP_SYS_ADMIN");
        return;
    }

    let mgr = LinuxCgroupManager::new("/sys/fs/cgroup");

    // Create hierarchy
    mgr.create_hierarchy().unwrap();

    // Verify slices exist
    assert!(Path::new("/sys/fs/cgroup").join(slices::PACT_ROOT).exists());
    assert!(Path::new("/sys/fs/cgroup").join(slices::PACT_INFRA).exists());
    assert!(Path::new("/sys/fs/cgroup").join(slices::PACT_GPU).exists());
    assert!(Path::new("/sys/fs/cgroup").join(slices::PACT_NETWORK).exists());
    assert!(Path::new("/sys/fs/cgroup").join(slices::WORKLOAD_ROOT).exists());

    // Create a scope with resource limits
    let limits = ResourceLimits {
        memory_max: Some(128 * 1024 * 1024), // 128 MB
        cpu_weight: Some(200),
        io_max: None,
    };
    let handle = mgr.create_scope(slices::PACT_INFRA, "e2e-test-svc", &limits).unwrap();

    // Verify scope exists
    let scope_path = Path::new("/sys/fs/cgroup").join(&handle.path);
    assert!(scope_path.exists());

    // Verify memory limit was set
    let mem_max = fs::read_to_string(scope_path.join("memory.max")).unwrap();
    assert_eq!(mem_max.trim(), "134217728"); // 128 MB

    // Verify cpu weight was set
    let cpu_w = fs::read_to_string(scope_path.join("cpu.weight")).unwrap();
    assert_eq!(cpu_w.trim(), "200");

    // Read metrics
    let metrics = mgr.read_metrics(&handle.path).unwrap();
    assert_eq!(metrics.nr_processes, 0);
    assert!(metrics.memory_current < 128 * 1024 * 1024);

    // Scope should be empty
    assert!(mgr.is_scope_empty(&handle).unwrap());

    // Destroy scope
    mgr.destroy_scope(&handle).unwrap();
    assert!(!scope_path.exists());

    // Cleanup: remove slices we created
    for slice in [
        slices::PACT_AUDIT,
        slices::PACT_GPU,
        slices::PACT_NETWORK,
        slices::PACT_INFRA,
        slices::PACT_ROOT,
        slices::WORKLOAD_ROOT,
    ] {
        let path = Path::new("/sys/fs/cgroup").join(slice);
        if path.exists() {
            let _ = fs::remove_dir(&path);
        }
    }
}

#[tokio::test]
#[ignore = "requires root/CAP_SYS_ADMIN on Linux"]
async fn cgroup_permission_denied_for_workload_slice() {
    if !has_cgroup_v2() || !can_create_cgroups() {
        eprintln!("SKIP: requires cgroup v2 + root/CAP_SYS_ADMIN");
        return;
    }

    let mgr = LinuxCgroupManager::new("/sys/fs/cgroup");
    mgr.create_hierarchy().unwrap();

    // Attempting to create scope in workload.slice should be denied (RI1)
    let err = mgr
        .create_scope(slices::WORKLOAD_ROOT, "should-fail", &ResourceLimits::default())
        .unwrap_err();

    assert!(
        matches!(err, hpc_node::CgroupError::PermissionDenied { .. }),
        "expected PermissionDenied, got {err:?}"
    );

    // Cleanup
    for slice in [
        slices::PACT_AUDIT,
        slices::PACT_GPU,
        slices::PACT_NETWORK,
        slices::PACT_INFRA,
        slices::PACT_ROOT,
        slices::WORKLOAD_ROOT,
    ] {
        let path = Path::new("/sys/fs/cgroup").join(slice);
        if path.exists() {
            let _ = fs::remove_dir(&path);
        }
    }
}

#[tokio::test]
#[ignore = "requires root/CAP_SYS_ADMIN on Linux"]
async fn cgroup_scope_with_process() {
    if !has_cgroup_v2() || !can_create_cgroups() {
        eprintln!("SKIP: requires cgroup v2 + root/CAP_SYS_ADMIN");
        return;
    }

    let mgr = LinuxCgroupManager::new("/sys/fs/cgroup");
    mgr.create_hierarchy().unwrap();

    let handle =
        mgr.create_scope(slices::PACT_INFRA, "e2e-proc-test", &ResourceLimits::default()).unwrap();

    // Spawn a sleep process in the cgroup
    let scope_path = Path::new("/sys/fs/cgroup").join(&handle.path);
    let mut child = Command::new("sleep").arg("300").spawn().expect("spawn sleep");

    // Move process into the cgroup
    let pid = child.id();
    fs::write(scope_path.join("cgroup.procs"), pid.to_string()).unwrap();

    // Scope should not be empty
    assert!(!mgr.is_scope_empty(&handle).unwrap());

    // Read metrics — should show 1 process
    let metrics = mgr.read_metrics(&handle.path).unwrap();
    assert_eq!(metrics.nr_processes, 1);

    // Destroy scope (should kill the process via cgroup.kill)
    mgr.destroy_scope(&handle).unwrap();

    // Process should be dead
    let status = child.wait().expect("wait for child");
    assert!(!status.success(), "process should have been killed");

    // Cleanup
    for slice in [
        slices::PACT_AUDIT,
        slices::PACT_GPU,
        slices::PACT_NETWORK,
        slices::PACT_INFRA,
        slices::PACT_ROOT,
        slices::WORKLOAD_ROOT,
    ] {
        let path = Path::new("/sys/fs/cgroup").join(slice);
        if path.exists() {
            let _ = fs::remove_dir(&path);
        }
    }
}

#[tokio::test]
#[ignore = "requires root/CAP_SYS_ADMIN"]
async fn oom_score_adj_protection() {
    // Verify OOM protection works
    pact_agent::isolation::protect_from_oom().unwrap();

    let score = fs::read_to_string("/proc/self/oom_score_adj").unwrap();
    assert_eq!(score.trim(), "-1000");
}
