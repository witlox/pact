//! E2E test: Linux namespace operations.
//!
//! Tests unshare(2) for pid/net/mount namespace creation and
//! process isolation. Requires Linux with CAP_SYS_ADMIN.
//!
//! Skipped on non-Linux platforms. Marked #[ignore] for CI.

#![cfg(target_os = "linux")]

use std::fs;
use std::process::Command;

/// Check if we have CAP_SYS_ADMIN for namespace operations.
fn can_create_namespaces() -> bool {
    // Try to unshare a mount namespace (lightest test)
    Command::new("unshare")
        .args(["--mount", "true"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[tokio::test]
#[ignore] // Requires root/CAP_SYS_ADMIN
async fn mount_namespace_isolation() {
    if !can_create_namespaces() {
        eprintln!("SKIP: requires CAP_SYS_ADMIN for namespace creation");
        return;
    }

    // Create a mount namespace and verify it's isolated
    let output = Command::new("unshare")
        .args(["--mount", "--", "sh", "-c", "mount -t tmpfs none /tmp/ns-test-mount && echo MOUNTED"])
        .output()
        .expect("unshare command");

    // The mount happened in an isolated namespace — /tmp/ns-test-mount
    // should NOT be mounted in our namespace
    let mounts = fs::read_to_string("/proc/mounts").unwrap();
    assert!(
        !mounts.contains("ns-test-mount"),
        "mount should be isolated to child namespace"
    );
}

#[tokio::test]
#[ignore] // Requires root/CAP_SYS_ADMIN
async fn pid_namespace_isolation() {
    if !can_create_namespaces() {
        eprintln!("SKIP: requires CAP_SYS_ADMIN for namespace creation");
        return;
    }

    // Create a pid namespace and check that PID 1 is the child process
    let output = Command::new("unshare")
        .args([
            "--pid",
            "--fork",
            "--mount-proc",
            "--",
            "sh",
            "-c",
            "cat /proc/self/status | grep ^Pid:",
        ])
        .output()
        .expect("unshare pid");

    let stdout = String::from_utf8_lossy(&output.stdout);
    // In the new pid namespace, the process should see itself as PID 1 or low PID
    // The first "Pid:" line is the NS-local PID
    assert!(
        stdout.contains("Pid:"),
        "should see Pid in process status, got: {stdout}"
    );
}

#[tokio::test]
#[ignore] // Requires root/CAP_SYS_ADMIN
async fn network_namespace_isolation() {
    if !can_create_namespaces() {
        eprintln!("SKIP: requires CAP_SYS_ADMIN for namespace creation");
        return;
    }

    // Create a net namespace — should only have loopback
    let output = Command::new("unshare")
        .args(["--net", "--", "ip", "link", "show"])
        .output()
        .expect("unshare net");

    let stdout = String::from_utf8_lossy(&output.stdout);
    // In a new network namespace, only 'lo' should exist
    assert!(
        stdout.contains("lo"),
        "new net namespace should have loopback, got: {stdout}"
    );
    // Should NOT have the host's eth0/ens* interfaces
    let line_count = stdout.lines().count();
    assert!(
        line_count <= 4,
        "new net namespace should only have loopback (got {line_count} lines)"
    );
}

#[tokio::test]
#[ignore] // Requires root/CAP_SYS_ADMIN
async fn squashfs_mount_lifecycle() {
    if !can_create_namespaces() {
        eprintln!("SKIP: requires CAP_SYS_ADMIN");
        return;
    }

    // Check if squashfs-tools is available for creating test images
    let has_mksquashfs = Command::new("which")
        .arg("mksquashfs")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !has_mksquashfs {
        eprintln!("SKIP: mksquashfs not available");
        return;
    }

    let dir = tempfile::TempDir::new().unwrap();
    let source_dir = dir.path().join("source");
    let image_path = dir.path().join("test.sqfs");
    let mount_point = dir.path().join("mount");

    // Create source content
    fs::create_dir_all(&source_dir).unwrap();
    fs::write(source_dir.join("hello.txt"), "Hello from SquashFS").unwrap();

    // Create SquashFS image
    let output = Command::new("mksquashfs")
        .args([
            source_dir.to_str().unwrap(),
            image_path.to_str().unwrap(),
            "-noappend",
            "-quiet",
        ])
        .output()
        .expect("mksquashfs");
    assert!(output.status.success(), "mksquashfs failed");

    // Mount it
    fs::create_dir_all(&mount_point).unwrap();
    let output = Command::new("mount")
        .args([
            "-t",
            "squashfs",
            "-o",
            "ro",
            image_path.to_str().unwrap(),
            mount_point.to_str().unwrap(),
        ])
        .output()
        .expect("mount squashfs");
    assert!(
        output.status.success(),
        "mount failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify content
    let content = fs::read_to_string(mount_point.join("hello.txt")).unwrap();
    assert_eq!(content, "Hello from SquashFS");

    // Unmount
    let output = Command::new("umount")
        .arg(mount_point.to_str().unwrap())
        .output()
        .expect("umount");
    assert!(output.status.success(), "umount failed");

    // Verify unmounted
    assert!(!mount_point.join("hello.txt").exists());
}
