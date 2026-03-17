//! Mount refcounting for shared uenv images.
//!
//! Multiple allocations can share one SquashFS mount. Refcounting tracks
//! active consumers. Lazy unmount with configurable hold time (WI3).
//!
//! # Invariants
//!
//! - WI2: refcount == active allocations using the mount. Never negative.
//! - WI3: lazy unmount with hold timer. Emergency force-unmount overrides.
//! - WI6: reconstruct from /proc/mounts + journal state on restart.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use tracing::{debug, info, warn};

/// State of a refcounted mount.
#[derive(Debug)]
pub struct MountRefState {
    /// Image path (e.g., "/images/pytorch-2.5.sqfs").
    pub image_path: String,
    /// Where the image is mounted.
    pub mount_point: String,
    /// Current reference count.
    pub refcount: u32,
    /// When hold timer started (if refcount == 0).
    pub hold_start: Option<Instant>,
}

/// Mount reference manager.
pub struct MountRefManager {
    /// Active mounts keyed by image path.
    mounts: HashMap<String, MountRefState>,
    /// Hold time before unmounting after refcount reaches zero.
    hold_duration: Duration,
    /// Mount base directory.
    mount_base: String,
}

impl MountRefManager {
    /// Create a new mount ref manager.
    #[must_use]
    pub fn new(mount_base: &str, hold_duration_secs: u64) -> Self {
        Self {
            mounts: HashMap::new(),
            hold_duration: Duration::from_secs(hold_duration_secs),
            mount_base: mount_base.to_string(),
        }
    }

    /// Acquire a reference to a uenv mount.
    ///
    /// If this is the first reference, the image would be mounted (on Linux).
    /// Returns the mount point.
    pub fn acquire(&mut self, image_path: &str) -> anyhow::Result<String> {
        if let Some(state) = self.mounts.get_mut(image_path) {
            state.refcount += 1;
            state.hold_start = None; // Cancel hold timer
            debug!(
                image = %image_path,
                refcount = state.refcount,
                "mount ref acquired (existing)"
            );
            return Ok(state.mount_point.clone());
        }

        // First reference — mount the image
        let mount_point = format!(
            "{}/{}",
            self.mount_base,
            image_path.rsplit('/').next().unwrap_or(image_path).replace(".sqfs", "")
        );

        // On Linux: would call mount(2) here
        // For now: just track the mount
        info!(
            image = %image_path,
            mount_point = %mount_point,
            "mounting uenv image (first reference)"
        );

        self.mounts.insert(
            image_path.to_string(),
            MountRefState {
                image_path: image_path.to_string(),
                mount_point: mount_point.clone(),
                refcount: 1,
                hold_start: None,
            },
        );

        Ok(mount_point)
    }

    /// Release a reference to a mount.
    ///
    /// Decrements refcount. When it reaches zero, starts the hold timer.
    pub fn release(&mut self, image_path: &str) {
        if let Some(state) = self.mounts.get_mut(image_path) {
            assert!(state.refcount > 0, "WI2: refcount must never go negative for {image_path}");
            state.refcount -= 1;
            debug!(
                image = %image_path,
                refcount = state.refcount,
                "mount ref released"
            );

            if state.refcount == 0 {
                state.hold_start = Some(Instant::now());
                info!(
                    image = %image_path,
                    hold_secs = self.hold_duration.as_secs(),
                    "refcount zero — hold timer started"
                );
            }
        }
    }

    /// Force-unmount regardless of refcount or hold timer (emergency only).
    pub fn force_unmount(&mut self, image_path: &str) {
        if let Some(state) = self.mounts.remove(image_path) {
            warn!(
                image = %image_path,
                refcount = state.refcount,
                "force unmount (emergency)"
            );
            // On Linux: would call umount2(MNT_FORCE) here
        }
    }

    /// Check for expired hold timers and unmount those images.
    ///
    /// Called periodically by the supervision loop (idle tick).
    pub fn check_expired_holds(&mut self) -> Vec<String> {
        let mut expired = Vec::new();

        for (path, state) in &self.mounts {
            if state.refcount == 0 {
                if let Some(start) = state.hold_start {
                    if start.elapsed() >= self.hold_duration {
                        expired.push(path.clone());
                    }
                }
            }
        }

        for path in &expired {
            if let Some(state) = self.mounts.remove(path) {
                info!(
                    image = %path,
                    mount_point = %state.mount_point,
                    "hold timer expired — unmounting"
                );
                // On Linux: would call umount(2) here
            }
        }

        expired
    }

    /// Reconstruct refcounts from known active allocations.
    ///
    /// Called on agent restart (WI6). Takes a list of (image_path, allocation_count)
    /// pairs derived from correlating /proc/mounts with journal state.
    pub fn reconstruct(&mut self, active_mounts: &[(&str, u32)]) {
        for (image_path, count) in active_mounts {
            let mount_point = format!(
                "{}/{}",
                self.mount_base,
                image_path.rsplit('/').next().unwrap_or(image_path).replace(".sqfs", "")
            );

            self.mounts.insert(
                (*image_path).to_string(),
                MountRefState {
                    image_path: (*image_path).to_string(),
                    mount_point,
                    refcount: *count,
                    hold_start: if *count == 0 { Some(Instant::now()) } else { None },
                },
            );
        }
        info!(mounts = self.mounts.len(), "mount refcounts reconstructed");
    }

    /// Get the current state of all mounts.
    #[must_use]
    pub fn states(&self) -> Vec<&MountRefState> {
        self.mounts.values().collect()
    }

    /// Get refcount for a specific image.
    #[must_use]
    pub fn refcount(&self, image_path: &str) -> Option<u32> {
        self.mounts.get(image_path).map(|s| s.refcount)
    }

    /// Total number of tracked mounts.
    #[must_use]
    pub fn mount_count(&self) -> usize {
        self.mounts.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_first_mount() {
        let mut mgr = MountRefManager::new("/run/pact/uenv", 60);
        let mp = mgr.acquire("pytorch-2.5.sqfs").unwrap();
        assert!(mp.contains("pytorch-2.5"));
        assert_eq!(mgr.refcount("pytorch-2.5.sqfs"), Some(1));
    }

    #[test]
    fn acquire_shared_mount_increments() {
        let mut mgr = MountRefManager::new("/run/pact/uenv", 60);
        mgr.acquire("pytorch-2.5.sqfs").unwrap();
        mgr.acquire("pytorch-2.5.sqfs").unwrap();
        assert_eq!(mgr.refcount("pytorch-2.5.sqfs"), Some(2));
    }

    #[test]
    fn release_decrements() {
        let mut mgr = MountRefManager::new("/run/pact/uenv", 60);
        mgr.acquire("pytorch-2.5.sqfs").unwrap();
        mgr.acquire("pytorch-2.5.sqfs").unwrap();
        mgr.release("pytorch-2.5.sqfs");
        assert_eq!(mgr.refcount("pytorch-2.5.sqfs"), Some(1));
    }

    #[test]
    fn release_to_zero_starts_hold_timer() {
        let mut mgr = MountRefManager::new("/run/pact/uenv", 60);
        mgr.acquire("pytorch-2.5.sqfs").unwrap();
        mgr.release("pytorch-2.5.sqfs");
        assert_eq!(mgr.refcount("pytorch-2.5.sqfs"), Some(0));
        // Mount still exists (hold timer)
        assert_eq!(mgr.mount_count(), 1);
    }

    #[test]
    fn reacquire_during_hold_cancels_timer() {
        let mut mgr = MountRefManager::new("/run/pact/uenv", 60);
        mgr.acquire("pytorch-2.5.sqfs").unwrap();
        mgr.release("pytorch-2.5.sqfs");
        // Reacquire during hold
        mgr.acquire("pytorch-2.5.sqfs").unwrap();
        assert_eq!(mgr.refcount("pytorch-2.5.sqfs"), Some(1));
        let state = &mgr.mounts["pytorch-2.5.sqfs"];
        assert!(state.hold_start.is_none()); // Timer cancelled
    }

    #[test]
    fn expired_hold_unmounts() {
        let mut mgr = MountRefManager::new("/run/pact/uenv", 0); // 0s hold = immediate
        mgr.acquire("pytorch-2.5.sqfs").unwrap();
        mgr.release("pytorch-2.5.sqfs");
        std::thread::sleep(Duration::from_millis(10));

        let expired = mgr.check_expired_holds();
        assert_eq!(expired.len(), 1);
        assert_eq!(mgr.mount_count(), 0); // Unmounted
    }

    #[test]
    fn non_expired_hold_keeps_mount() {
        let mut mgr = MountRefManager::new("/run/pact/uenv", 3600); // 1h hold
        mgr.acquire("pytorch-2.5.sqfs").unwrap();
        mgr.release("pytorch-2.5.sqfs");

        let expired = mgr.check_expired_holds();
        assert!(expired.is_empty());
        assert_eq!(mgr.mount_count(), 1); // Still there
    }

    #[test]
    fn force_unmount_removes_immediately() {
        let mut mgr = MountRefManager::new("/run/pact/uenv", 3600);
        mgr.acquire("pytorch-2.5.sqfs").unwrap();
        mgr.acquire("pytorch-2.5.sqfs").unwrap();
        // Force unmount even with refcount=2
        mgr.force_unmount("pytorch-2.5.sqfs");
        assert_eq!(mgr.mount_count(), 0);
    }

    #[test]
    fn reconstruct_state() {
        let mut mgr = MountRefManager::new("/run/pact/uenv", 60);
        mgr.reconstruct(&[("pytorch-2.5.sqfs", 2), ("jax-0.4.sqfs", 0)]);
        assert_eq!(mgr.mount_count(), 2);
        assert_eq!(mgr.refcount("pytorch-2.5.sqfs"), Some(2));
        assert_eq!(mgr.refcount("jax-0.4.sqfs"), Some(0));
        // Zero-refcount mount should have hold timer
        assert!(mgr.mounts["jax-0.4.sqfs"].hold_start.is_some());
    }

    #[test]
    #[should_panic(expected = "WI2")]
    fn release_below_zero_panics() {
        let mut mgr = MountRefManager::new("/run/pact/uenv", 60);
        mgr.acquire("test.sqfs").unwrap();
        mgr.release("test.sqfs");
        mgr.release("test.sqfs"); // Should panic — refcount would go negative
    }

    #[test]
    fn multiple_images_independent() {
        let mut mgr = MountRefManager::new("/run/pact/uenv", 60);
        mgr.acquire("pytorch-2.5.sqfs").unwrap();
        mgr.acquire("jax-0.4.sqfs").unwrap();
        mgr.acquire("pytorch-2.5.sqfs").unwrap();

        assert_eq!(mgr.refcount("pytorch-2.5.sqfs"), Some(2));
        assert_eq!(mgr.refcount("jax-0.4.sqfs"), Some(1));
        assert_eq!(mgr.mount_count(), 2);
    }
}
