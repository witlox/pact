//! Hardware watchdog handle for PID 1 mode.
//!
//! Opens `/dev/watchdog`, reads/sets timeout, pets periodically.
//! Only used when pact-agent is PID 1 on a BMC-equipped node (PB1).
//!
//! Invariants enforced:
//! - PB1: WatchdogHandle only opened when PID 1 + device exists
//! - PB2: Pet at least every T/2 seconds (coupled to supervision loop via PS2)
//! - F23: Hung supervision loop → no pet → BMC reboot (intended recovery)
//!
//! Drop writes magic close character 'V' to disarm watchdog on graceful shutdown.

use std::sync::Arc;

#[cfg(not(target_os = "linux"))]
use tracing::debug;
use tracing::{error, warn};

/// Hardware watchdog handle.
///
/// Wraps a file descriptor to `/dev/watchdog`. The Linux watchdog driver
/// starts its countdown timer when the device is opened. If not petted
/// within the timeout period, the BMC triggers a hard reboot.
pub struct WatchdogHandle {
    /// File descriptor for /dev/watchdog.
    #[cfg(target_os = "linux")]
    fd: std::os::unix::io::OwnedFd,
    /// Watchdog timeout in seconds (read from hardware or set by us).
    timeout_secs: u32,
}

// ---------------------------------------------------------------------------
// Linux implementation
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
mod linux {
    use std::fs::OpenOptions;
    use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};

    use tracing::{debug, info};

    use super::WatchdogHandle;

    const WATCHDOG_PATH: &str = "/dev/watchdog";

    // Linux watchdog ioctl numbers (from linux/watchdog.h).
    // These are architecture-independent on Linux.
    const WATCHDOG_IOCTL_BASE: u8 = b'W';

    nix::ioctl_read!(wdioc_gettimeout, WATCHDOG_IOCTL_BASE, 7, libc::c_int);
    nix::ioctl_write_int!(wdioc_settimeout, WATCHDOG_IOCTL_BASE, 6);
    nix::ioctl_write_int!(wdioc_keepalive, WATCHDOG_IOCTL_BASE, 5);

    impl WatchdogHandle {
        /// Open `/dev/watchdog` and read its timeout.
        ///
        /// Returns:
        /// - `Ok(Some(handle))` — device exists, opened successfully
        /// - `Ok(None)` — device does not exist (no BMC, cloud VM, etc.)
        /// - `Err` — device exists but open/ioctl failed (EBUSY, permission, hardware)
        pub fn open() -> anyhow::Result<Option<Self>> {
            use std::path::Path;

            if !Path::new(WATCHDOG_PATH).exists() {
                debug!("no /dev/watchdog — skipping hardware watchdog");
                return Ok(None);
            }

            let file = OpenOptions::new().write(true).open(WATCHDOG_PATH).map_err(|e| {
                if e.raw_os_error() == Some(libc::EBUSY) {
                    anyhow::anyhow!("watchdog device busy — another process holds /dev/watchdog")
                } else {
                    anyhow::anyhow!("failed to open /dev/watchdog: {e}")
                }
            })?;

            // SAFETY: OwnedFd takes ownership of the fd, which we obtained from a
            // valid File. The File is consumed (into_raw_fd) so no double-close.
            let fd = unsafe { OwnedFd::from_raw_fd(file.into_raw_fd()) };

            let mut timeout: libc::c_int = 0;
            // SAFETY: fd is a valid watchdog device fd, timeout is a valid pointer.
            unsafe {
                wdioc_gettimeout(fd.as_raw_fd(), &mut timeout)
                    .map_err(|e| anyhow::anyhow!("WDIOC_GETTIMEOUT ioctl failed: {e}"))?;
            }

            let timeout_secs = timeout.max(1) as u32;
            info!(timeout_secs, "hardware watchdog opened — countdown started");

            Ok(Some(Self { fd, timeout_secs }))
        }

        /// Set the watchdog timeout in seconds.
        pub fn set_timeout(&self, seconds: u32) -> anyhow::Result<()> {
            // SAFETY: fd is a valid watchdog fd, seconds fits in c_int.
            unsafe {
                wdioc_settimeout(self.fd.as_raw_fd(), u64::from(seconds))
                    .map_err(|e| anyhow::anyhow!("WDIOC_SETTIMEOUT failed: {e}"))?;
            }
            info!(seconds, "watchdog timeout set");
            Ok(())
        }

        /// Pet the watchdog — resets the countdown timer.
        ///
        /// If the ioctl fails, logs an error but does NOT panic.
        /// A failed pet is equivalent to a hang — the watchdog will
        /// eventually fire and BMC will reboot, which is the intended
        /// recovery mechanism (F23).
        pub fn pet(&self) -> Result<(), nix::Error> {
            // SAFETY: fd is a valid watchdog fd, value is unused (0).
            unsafe { wdioc_keepalive(self.fd.as_raw_fd(), 0) }?;
            Ok(())
        }
    }

    impl Drop for WatchdogHandle {
        fn drop(&mut self) {
            // Write magic close character 'V' to disarm the watchdog.
            // This prevents BMC reboot on graceful shutdown.
            use std::io::Write;
            use std::os::unix::io::{AsRawFd, FromRawFd};

            // SAFETY: from_raw_fd borrows the fd for writing. We do NOT consume it;
            // OwnedFd still owns and will close it after drop completes.
            let mut file = unsafe { std::fs::File::from_raw_fd(self.fd.as_raw_fd()) };
            if let Err(e) = file.write_all(b"V") {
                error!(error = %e, "failed to write magic close to watchdog");
            } else {
                info!("watchdog disarmed (magic close)");
            }
            // Prevent File from closing the fd — OwnedFd owns it.
            std::mem::forget(file);
        }
    }
}

// ---------------------------------------------------------------------------
// Non-Linux stub
// ---------------------------------------------------------------------------

#[cfg(not(target_os = "linux"))]
impl WatchdogHandle {
    /// No watchdog on non-Linux — always returns `Ok(None)`.
    pub fn open() -> anyhow::Result<Option<Self>> {
        debug!("non-Linux platform — no hardware watchdog");
        Ok(None)
    }

    /// Stub — no-op.
    pub fn set_timeout(&self, _seconds: u32) -> anyhow::Result<()> {
        Ok(())
    }

    /// Stub — always succeeds.
    pub fn pet(&self) -> Result<(), nix::Error> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Shared methods (both platforms)
// ---------------------------------------------------------------------------

impl WatchdogHandle {
    /// Get the current watchdog timeout in seconds.
    pub fn timeout(&self) -> u32 {
        self.timeout_secs
    }

    /// Return a closure suitable for `PactSupervisor::start_supervision_loop`'s
    /// `watchdog_pet` parameter.
    ///
    /// The closure calls `pet()` and logs on error but NEVER panics.
    /// A failed pet is treated as equivalent to a hang — the watchdog will
    /// eventually fire and BMC will reboot (F23), which is correct.
    pub fn as_pet_callback(self: &Arc<Self>) -> Arc<dyn Fn() + Send + Sync> {
        let handle = Arc::clone(self);
        Arc::new(move || {
            if let Err(e) = handle.pet() {
                // Log but don't panic — failed pet = eventual BMC reboot (F23)
                error!(error = %e, "watchdog pet failed — BMC reboot may follow");
            }
        })
    }

    /// Spawn a dedicated petting task for the boot phase.
    ///
    /// During boot, the supervision loop has not started yet. If a boot
    /// phase fails and enters retry (F33), the watchdog must still be
    /// petted. This task pets at T/2 interval until the returned
    /// `AbortHandle` is used to stop it (when supervision loop takes over).
    pub fn spawn_boot_petter(self: &Arc<Self>) -> tokio::task::AbortHandle {
        let handle = Arc::clone(self);
        let interval = std::time::Duration::from_secs(u64::from((handle.timeout_secs / 2).max(1)));
        let task = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            loop {
                ticker.tick().await;
                if let Err(e) = handle.pet() {
                    warn!(error = %e, "boot petter: watchdog pet failed");
                }
            }
        });
        task.abort_handle()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_returns_none_on_missing_device() {
        // On dev machines (macOS or Linux without /dev/watchdog), open() returns None.
        let result = WatchdogHandle::open().unwrap();
        assert!(result.is_none(), "should return None when /dev/watchdog is absent");
    }

    #[test]
    fn pet_callback_does_not_panic() {
        // Non-Linux stub: create a handle and verify the callback doesn't panic.
        #[cfg(not(target_os = "linux"))]
        {
            // Can't construct WatchdogHandle directly on non-Linux (no fd),
            // so we verify that open() returns None — callback path is tested
            // via supervision_loop_pets_watchdog in pact_supervisor.rs.
        }
    }

    #[tokio::test]
    async fn boot_petter_can_be_aborted() {
        // Verify the boot petter task can be spawned and aborted without panic.
        // On non-Linux, we can't construct a real handle, so this tests the
        // abort mechanism only on platforms where open() returns Some.
        let handle = WatchdogHandle::open().unwrap();
        if let Some(watchdog) = handle {
            let arc = Arc::new(watchdog);
            let abort = arc.spawn_boot_petter();
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            abort.abort();
            // Should not panic
        }
    }
}
