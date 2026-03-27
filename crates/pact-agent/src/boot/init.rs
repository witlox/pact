//! Early init for PID 1 mode.
//!
//! Mounts pseudofilesystems, sets up console, spawns async zombie reaper.
//! Only executed when pact-agent IS PID 1.
//!
//! Invariants enforced:
//! - PB0: Pseudofs mounted before any /proc readers (OOM protection, sysctl)
//! - PB3: Phase 0 prerequisite — mount failure blocks all subsequent phases
//! - PS3: Zombie reaping for processes reparented to PID 1

#[cfg(not(target_os = "linux"))]
use tracing::debug;

/// Early init for PID 1 mode.
pub struct PlatformInit;

impl PlatformInit {
    /// Returns true if this process is PID 1.
    #[cfg(target_os = "linux")]
    pub fn is_pid1() -> bool {
        nix::unistd::getpid() == nix::unistd::Pid::from_raw(1)
    }

    #[cfg(not(target_os = "linux"))]
    pub fn is_pid1() -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// Linux implementation
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
mod linux {
    use std::path::Path;

    use nix::mount::{mount, MsFlags};
    use tracing::{debug, info, warn};

    use super::PlatformInit;

    /// A pseudofilesystem to mount, in dependency order.
    struct PseudoMount {
        source: &'static str,
        target: &'static str,
        fstype: &'static str,
        flags: MsFlags,
        data: &'static str,
    }

    /// Pseudofilesystems in mount order. Dependencies require this sequence:
    /// 1. /proc — needed before anything reads /proc (OOM, sysctl, mount checks)
    /// 2. /sys — needed for device/cgroup discovery
    /// 3. /dev — devtmpfs, NOT tmpfs (preserves kernel device nodes)
    /// 4. /dev/pts — pseudo-terminals for pact shell
    /// 5. /dev/shm — POSIX shared memory
    /// 6. /run — runtime state (/run/pact/)
    /// 7. /tmp — temporary files
    const PSEUDO_MOUNTS: &[PseudoMount] = &[
        PseudoMount {
            source: "proc",
            target: "/proc",
            fstype: "proc",
            flags: MsFlags::MS_NOSUID,
            data: "",
        },
        PseudoMount {
            source: "sysfs",
            target: "/sys",
            fstype: "sysfs",
            flags: MsFlags::MS_NOSUID,
            data: "",
        },
        PseudoMount {
            source: "devtmpfs",
            target: "/dev",
            fstype: "devtmpfs",
            flags: MsFlags::MS_NOSUID,
            data: "mode=0755",
        },
        PseudoMount {
            source: "devpts",
            target: "/dev/pts",
            fstype: "devpts",
            flags: MsFlags::MS_NOSUID,
            data: "mode=0620,gid=5",
        },
        PseudoMount {
            source: "tmpfs",
            target: "/dev/shm",
            fstype: "tmpfs",
            flags: MsFlags::MS_NOSUID,
            data: "mode=1777",
        },
        PseudoMount {
            source: "tmpfs",
            target: "/run",
            fstype: "tmpfs",
            flags: MsFlags::MS_NOSUID,
            data: "mode=0755",
        },
        PseudoMount {
            source: "tmpfs",
            target: "/tmp",
            fstype: "tmpfs",
            flags: MsFlags::MS_NOSUID,
            data: "mode=1777",
        },
    ];

    impl PlatformInit {
        /// Mount essential pseudofilesystems.
        ///
        /// Idempotent: checks if target is already a mountpoint and skips
        /// if so. This handles the case where the kernel or initramfs
        /// already mounted some of these (e.g., `devtmpfs.mount=1`).
        ///
        /// /proc is mounted first because `is_mountpoint()` reads /proc/mounts
        /// for subsequent checks. The /proc mount itself uses a stat-based
        /// check (if /proc/self exists, proc is already mounted).
        pub fn mount_pseudofs() -> anyhow::Result<()> {
            for pm in PSEUDO_MOUNTS {
                let target = Path::new(pm.target);

                // Ensure target directory exists
                if !target.exists() {
                    std::fs::create_dir_all(target).map_err(|e| {
                        anyhow::anyhow!("failed to create mount target {}: {e}", pm.target)
                    })?;
                }

                // Skip if already mounted
                if is_mountpoint(pm.target) {
                    debug!(target = pm.target, "already mounted — skipping");
                    continue;
                }

                mount(Some(pm.source), pm.target, Some(pm.fstype), pm.flags, Some(pm.data))
                    .map_err(|e| {
                        anyhow::anyhow!("failed to mount {} on {}: {e}", pm.fstype, pm.target)
                    })?;

                info!(target = pm.target, fstype = pm.fstype, "mounted");
            }

            Ok(())
        }

        /// Set up /dev/console as stdin/stdout/stderr.
        pub fn setup_console() -> anyhow::Result<()> {
            use std::os::unix::io::AsRawFd;

            let console_path = "/dev/console";
            if !Path::new(console_path).exists() {
                warn!("/dev/console not found — skipping console setup");
                return Ok(());
            }

            let console = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(console_path)
                .map_err(|e| anyhow::anyhow!("failed to open /dev/console: {e}"))?;

            let fd = console.as_raw_fd();

            // Redirect stdin, stdout, stderr to console.
            // SAFETY: dup2 is safe with valid fds. fd is valid (just opened).
            unsafe {
                libc::dup2(fd, libc::STDIN_FILENO);
                libc::dup2(fd, libc::STDOUT_FILENO);
                libc::dup2(fd, libc::STDERR_FILENO);
            }

            // Close the original fd if it's not one of 0/1/2.
            if fd > 2 {
                drop(console);
            } else {
                std::mem::forget(console);
            }

            info!("console redirected to /dev/console");
            Ok(())
        }

        /// Spawn an async zombie reaper task using tokio's signal infrastructure.
        ///
        /// Uses `tokio::signal::unix::signal(SignalKind::child())` — cooperates
        /// with tokio's internal SIGCHLD handling. Does NOT install a raw
        /// sigaction handler (which would break `Child::try_wait()` in the
        /// supervision loop).
        pub fn spawn_zombie_reaper() -> anyhow::Result<tokio::task::AbortHandle> {
            use tokio::signal::unix::{signal, SignalKind};

            let mut sigchld = signal(SignalKind::child())
                .map_err(|e| anyhow::anyhow!("failed to register SIGCHLD stream: {e}"))?;

            let task = tokio::spawn(async move {
                loop {
                    sigchld.recv().await;
                    // Reap all available zombies (there may be multiple).
                    loop {
                        match nix::sys::wait::waitpid(
                            nix::unistd::Pid::from_raw(-1),
                            Some(nix::sys::wait::WaitPidFlag::WNOHANG),
                        ) {
                            Ok(nix::sys::wait::WaitStatus::StillAlive) => break,
                            Ok(status) => {
                                debug!(?status, "reaped zombie child");
                            }
                            Err(nix::errno::Errno::ECHILD) => break, // no children
                            Err(e) => {
                                warn!(error = %e, "waitpid error in zombie reaper");
                                break;
                            }
                        }
                    }
                }
            });

            info!("zombie reaper spawned");
            Ok(task.abort_handle())
        }

        /// Set hostname.
        pub fn set_hostname(hostname: &str) -> anyhow::Result<()> {
            nix::unistd::sethostname(hostname)
                .map_err(|e| anyhow::anyhow!("failed to set hostname to '{hostname}': {e}"))?;
            info!(hostname, "hostname set");
            Ok(())
        }
    }

    /// Check if a path is a mountpoint.
    ///
    /// For /proc specifically: checks if /proc/self exists (stat-based,
    /// no /proc/mounts dependency). For everything else: reads /proc/mounts.
    fn is_mountpoint(target: &str) -> bool {
        if target == "/proc" {
            // Can't read /proc/mounts if /proc isn't mounted yet.
            // Use the existence of /proc/self as a heuristic.
            return Path::new("/proc/self").exists();
        }

        // Read /proc/mounts and check if target appears as a mount point.
        match std::fs::read_to_string("/proc/mounts") {
            Ok(mounts) => mounts.lines().any(|line| line.split_whitespace().nth(1) == Some(target)),
            Err(_) => false, // /proc not mounted yet? Not a mountpoint.
        }
    }
}

// ---------------------------------------------------------------------------
// Non-Linux stubs
// ---------------------------------------------------------------------------

#[cfg(not(target_os = "linux"))]
impl PlatformInit {
    pub fn mount_pseudofs() -> anyhow::Result<()> {
        debug!("non-Linux platform — skipping pseudofs mount");
        Ok(())
    }

    pub fn setup_console() -> anyhow::Result<()> {
        debug!("non-Linux platform — skipping console setup");
        Ok(())
    }

    pub fn spawn_zombie_reaper() -> anyhow::Result<tokio::task::AbortHandle> {
        debug!("non-Linux platform — skipping zombie reaper");
        // Return a no-op abort handle via a dummy task.
        let task = tokio::spawn(std::future::pending::<()>());
        Ok(task.abort_handle())
    }

    pub fn set_hostname(_hostname: &str) -> anyhow::Result<()> {
        debug!("non-Linux platform — skipping hostname set");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_pid1_returns_false_in_tests() {
        // Tests never run as PID 1.
        assert!(!PlatformInit::is_pid1());
    }

    #[test]
    fn mount_pseudofs_succeeds_on_non_linux() {
        // On macOS dev, this is a no-op stub.
        #[cfg(not(target_os = "linux"))]
        {
            PlatformInit::mount_pseudofs().unwrap();
        }
    }

    #[tokio::test]
    async fn zombie_reaper_can_be_aborted() {
        let abort = PlatformInit::spawn_zombie_reaper().unwrap();
        // Give it a tick
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        abort.abort();
        // Should not panic
    }
}
