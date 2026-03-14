//! State observer — detects changes to system state.
//!
//! Three mechanisms (Linux): eBPF probes, inotify file watches, netlink sockets.
//! On macOS: `MockObserver` for development/testing.
//! All observers emit `ObserverEvent` to the drift evaluator.

use chrono::{DateTime, Utc};
use tokio::sync::mpsc;

#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
#[cfg(target_os = "linux")]
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "linux")]
use std::sync::Arc;

/// An event detected by any observer subsystem.
#[derive(Debug, Clone)]
pub struct ObserverEvent {
    /// Category: "mount", "file", "network", "service", "kernel", "package", "gpu"
    pub category: String,
    /// Path or identifier of the changed resource.
    pub path: String,
    /// Human-readable detail of the change.
    pub detail: String,
    /// When the change was detected.
    pub timestamp: DateTime<Utc>,
}

/// Trait for state observation backends.
#[async_trait::async_trait]
pub trait Observer: Send + Sync {
    /// Start observing and send events to the channel.
    async fn start(&self, tx: mpsc::Sender<ObserverEvent>) -> anyhow::Result<()>;

    /// Stop observing.
    async fn stop(&self) -> anyhow::Result<()>;
}

// ---------------------------------------------------------------------------
// InotifyObserver — filesystem change watcher (Linux only)
// ---------------------------------------------------------------------------

/// Watches filesystem paths for changes using Linux inotify.
///
/// Emits `ObserverEvent` with `category = "file"` for every modify, create,
/// delete, or move detected on the watched paths.
#[cfg(target_os = "linux")]
pub struct InotifyObserver {
    paths: Vec<PathBuf>,
    running: Arc<AtomicBool>,
}

#[cfg(target_os = "linux")]
impl InotifyObserver {
    /// Create a new inotify observer watching the given paths.
    pub fn new(paths: Vec<PathBuf>) -> Self {
        Self { paths, running: Arc::new(AtomicBool::new(false)) }
    }
}

#[cfg(target_os = "linux")]
#[async_trait::async_trait]
impl Observer for InotifyObserver {
    async fn start(&self, tx: mpsc::Sender<ObserverEvent>) -> anyhow::Result<()> {
        use nix::sys::inotify::{AddWatchFlags, InitFlags, Inotify};

        let inotify = Inotify::init(InitFlags::IN_NONBLOCK)?;
        let watch_flags = AddWatchFlags::IN_MODIFY
            | AddWatchFlags::IN_CREATE
            | AddWatchFlags::IN_DELETE
            | AddWatchFlags::IN_MOVED_FROM
            | AddWatchFlags::IN_MOVED_TO;

        // Map watch descriptors back to paths for event reporting.
        let mut wd_to_path = std::collections::HashMap::new();
        for path in &self.paths {
            let wd = inotify.add_watch(path, watch_flags)?;
            wd_to_path.insert(wd, path.clone());
        }

        self.running.store(true, Ordering::SeqCst);
        let running = Arc::clone(&self.running);
        let fd = inotify.as_raw_fd();

        tokio::spawn(async move {
            let async_fd = match tokio::io::unix::AsyncFd::new(fd) {
                Ok(f) => f,
                Err(e) => {
                    tracing::error!("InotifyObserver: failed to create AsyncFd: {e}");
                    return;
                }
            };

            while running.load(Ordering::SeqCst) {
                // Wait until the inotify fd is readable.
                let guard = match async_fd.readable().await {
                    Ok(g) => g,
                    Err(_) => break,
                };

                match inotify.read_events() {
                    Ok(events) => {
                        for ev in events {
                            let base = wd_to_path
                                .get(&ev.wd)
                                .map(|p| p.display().to_string())
                                .unwrap_or_default();

                            let full_path = if let Some(ref name) = ev.name {
                                format!("{}/{}", base, name.to_string_lossy())
                            } else {
                                base
                            };

                            let detail = describe_inotify_mask(ev.mask);

                            let observer_event = ObserverEvent {
                                category: "file".into(),
                                path: full_path,
                                detail,
                                timestamp: Utc::now(),
                            };

                            if tx.send(observer_event).await.is_err() {
                                // Receiver dropped — stop.
                                running.store(false, Ordering::SeqCst);
                                return;
                            }
                        }
                    }
                    Err(nix::errno::Errno::EAGAIN) => {
                        // No events ready — clear readiness and loop.
                    }
                    Err(e) => {
                        tracing::error!("InotifyObserver: read error: {e}");
                        break;
                    }
                }

                guard.clear_ready();
            }
        });

        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }
}

/// Translate an inotify event mask into a human-readable description.
#[cfg(target_os = "linux")]
fn describe_inotify_mask(mask: nix::sys::inotify::AddWatchFlags) -> String {
    use nix::sys::inotify::AddWatchFlags;

    let mut parts = Vec::new();
    if mask.contains(AddWatchFlags::IN_CREATE) {
        parts.push("created");
    }
    if mask.contains(AddWatchFlags::IN_DELETE) {
        parts.push("deleted");
    }
    if mask.contains(AddWatchFlags::IN_MODIFY) {
        parts.push("modified");
    }
    if mask.contains(AddWatchFlags::IN_MOVED_FROM) {
        parts.push("moved_from");
    }
    if mask.contains(AddWatchFlags::IN_MOVED_TO) {
        parts.push("moved_to");
    }
    if parts.is_empty() {
        format!("inotify event 0x{:x}", mask.bits())
    } else {
        parts.join(", ")
    }
}

// ---------------------------------------------------------------------------
// NetlinkObserver — network interface change monitor (Linux only)
// ---------------------------------------------------------------------------

/// Monitors network interface link changes using a NETLINK_ROUTE socket.
///
/// Emits `ObserverEvent` with `category = "network"` whenever a link
/// state change notification is received from the kernel.
#[cfg(target_os = "linux")]
pub struct NetlinkObserver {
    running: Arc<AtomicBool>,
}

#[cfg(target_os = "linux")]
impl NetlinkObserver {
    /// Create a new netlink observer.
    pub fn new() -> Self {
        Self { running: Arc::new(AtomicBool::new(false)) }
    }
}

#[cfg(target_os = "linux")]
impl Default for NetlinkObserver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "linux")]
#[async_trait::async_trait]
impl Observer for NetlinkObserver {
    async fn start(&self, tx: mpsc::Sender<ObserverEvent>) -> anyhow::Result<()> {
        use nix::sys::socket::{bind, socket, AddressFamily, NetlinkAddr, SockFlag, SockType};

        // RTMGRP_LINK = 1 — subscribe to link notifications.
        const RTMGRP_LINK: u32 = 1;

        let sock = socket(
            AddressFamily::Netlink,
            SockType::Datagram,
            SockFlag::SOCK_NONBLOCK | SockFlag::SOCK_CLOEXEC,
            None, // NETLINK_ROUTE is protocol 0, the default
        )?;

        let addr = NetlinkAddr::new(0, RTMGRP_LINK);
        bind(sock.as_raw_fd(), &addr)?;

        self.running.store(true, Ordering::SeqCst);
        let running = Arc::clone(&self.running);

        tokio::spawn(async move {
            let raw_fd = sock.as_raw_fd();
            let async_fd = match tokio::io::unix::AsyncFd::new(raw_fd) {
                Ok(f) => f,
                Err(e) => {
                    tracing::error!("NetlinkObserver: failed to create AsyncFd: {e}");
                    return;
                }
            };

            // Keep the OwnedFd alive for the duration of the task so `raw_fd` stays valid.
            let _sock_guard = sock;

            let mut buf = [0u8; 4096];

            while running.load(Ordering::SeqCst) {
                let guard = match async_fd.readable().await {
                    Ok(g) => g,
                    Err(_) => break,
                };

                match nix::unistd::read(&_sock_guard, &mut buf) {
                    Ok(n) if n > 0 => {
                        let event = ObserverEvent {
                            category: "network".into(),
                            path: "netlink".into(),
                            detail: format!("link change notification ({n} bytes)"),
                            timestamp: Utc::now(),
                        };
                        if tx.send(event).await.is_err() {
                            running.store(false, Ordering::SeqCst);
                            return;
                        }
                    }
                    Ok(_) => {
                        // EOF or zero-length read.
                    }
                    Err(nix::errno::Errno::EAGAIN) => {
                        // No data ready.
                    }
                    Err(e) => {
                        tracing::error!("NetlinkObserver: read error: {e}");
                        break;
                    }
                }

                guard.clear_ready();
            }
        });

        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }
}

/// Mock observer for development and testing on macOS.
pub struct MockObserver {
    events: Vec<ObserverEvent>,
}

impl MockObserver {
    pub fn new() -> Self {
        Self { events: vec![] }
    }

    /// Create a mock observer with pre-configured events to emit.
    pub fn with_events(events: Vec<ObserverEvent>) -> Self {
        Self { events }
    }
}

impl Default for MockObserver {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Observer for MockObserver {
    async fn start(&self, tx: mpsc::Sender<ObserverEvent>) -> anyhow::Result<()> {
        for event in &self.events {
            if tx.send(event.clone()).await.is_err() {
                break;
            }
        }
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pact_common::config::BlacklistConfig;
    use pact_common::types::DriftWeights;

    fn event(category: &str, path: &str, detail: &str) -> ObserverEvent {
        ObserverEvent {
            category: category.into(),
            path: path.into(),
            detail: detail.into(),
            timestamp: Utc::now(),
        }
    }

    #[tokio::test]
    async fn observer_events_flow_to_drift_evaluator() {
        // This is the real contract: observer emits events → drift evaluator accumulates them.
        let events = vec![
            event("kernel", "/etc/sysctl.conf", "shmmax changed"),
            event("mount", "/mnt/data", "new mount"),
            event("file", "/etc/pact/config.toml", "modified"),
        ];

        let observer = MockObserver::with_events(events);
        let (tx, mut rx) = mpsc::channel(10);
        observer.start(tx).await.unwrap();

        // Feed events through drift evaluator — this tests the real pipeline
        let mut evaluator =
            crate::drift::DriftEvaluator::new(BlacklistConfig::default(), DriftWeights::default());

        let mut count = 0;
        while let Ok(ev) = rx.try_recv() {
            evaluator.process_event(&ev);
            count += 1;
        }
        assert_eq!(count, 3);

        let drift = evaluator.drift_vector();
        assert_eq!(drift.kernel, 1.0);
        assert_eq!(drift.mounts, 1.0);
        assert_eq!(drift.files, 1.0);
        assert!(evaluator.magnitude() > 0.0);
    }

    #[tokio::test]
    async fn blacklisted_observer_events_produce_zero_drift() {
        // Observer emits events for blacklisted paths → drift should stay zero
        let events = vec![
            event("file", "/tmp/scratch/output.log", "created"),
            event("file", "/var/log/syslog", "rotated"),
            event("file", "/proc/cpuinfo", "read"),
            event("file", "/sys/class/net/eth0/speed", "changed"),
        ];

        let observer = MockObserver::with_events(events);
        let (tx, mut rx) = mpsc::channel(10);
        observer.start(tx).await.unwrap();

        let mut evaluator =
            crate::drift::DriftEvaluator::new(BlacklistConfig::default(), DriftWeights::default());

        while let Ok(ev) = rx.try_recv() {
            evaluator.process_event(&ev);
        }

        assert_eq!(evaluator.magnitude(), 0.0, "blacklisted events should not produce drift");
    }

    #[tokio::test]
    async fn observer_preserves_event_fields() {
        let ts = Utc::now();
        let events = vec![ObserverEvent {
            category: "gpu".into(),
            path: "GPU-0000:3b:00.0".into(),
            detail: "temperature threshold exceeded".into(),
            timestamp: ts,
        }];

        let observer = MockObserver::with_events(events);
        let (tx, mut rx) = mpsc::channel(10);
        observer.start(tx).await.unwrap();

        let received = rx.recv().await.unwrap();
        assert_eq!(received.category, "gpu");
        assert_eq!(received.path, "GPU-0000:3b:00.0");
        assert_eq!(received.detail, "temperature threshold exceeded");
        assert_eq!(received.timestamp, ts);
    }

    #[tokio::test]
    async fn observer_handles_dropped_receiver() {
        // If the receiver is dropped, observer should stop sending (not panic)
        let events = vec![
            event("file", "/etc/a", "a"),
            event("file", "/etc/b", "b"),
            event("file", "/etc/c", "c"),
        ];

        let observer = MockObserver::with_events(events);
        let (tx, rx) = mpsc::channel(1); // capacity 1
        drop(rx); // drop receiver before start

        // Should not panic — the send loop breaks on error
        let result = observer.start(tx).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn observer_events_all_seven_categories_produce_drift() {
        let events = vec![
            event("mount", "/mnt/scratch", "mounted"),
            event("file", "/etc/hostname", "changed"),
            event("network", "eth0", "link down"),
            event("service", "lattice-node-agent", "crashed"),
            event("kernel", "/etc/sysctl.conf", "modified"),
            event("package", "nvidia-driver", "upgraded"),
            event("gpu", "GPU-0", "degraded"),
        ];

        let observer = MockObserver::with_events(events);
        let (tx, mut rx) = mpsc::channel(10);
        observer.start(tx).await.unwrap();

        let mut evaluator = crate::drift::DriftEvaluator::new(
            BlacklistConfig { patterns: vec![] }, // no blacklist — count everything
            DriftWeights::default(),
        );

        while let Ok(ev) = rx.try_recv() {
            evaluator.process_event(&ev);
        }

        let drift = evaluator.drift_vector();
        assert_eq!(drift.mounts, 1.0);
        assert_eq!(drift.files, 1.0);
        assert_eq!(drift.network, 1.0);
        assert_eq!(drift.services, 1.0);
        assert_eq!(drift.kernel, 1.0);
        assert_eq!(drift.packages, 1.0);
        assert_eq!(drift.gpu, 1.0);
    }

    #[tokio::test]
    async fn unknown_category_events_produce_no_drift() {
        let events = vec![
            event("unknown_thing", "/etc/foo", "happened"),
            event("", "/whatever", "empty category"),
        ];

        let observer = MockObserver::with_events(events);
        let (tx, mut rx) = mpsc::channel(10);
        observer.start(tx).await.unwrap();

        let mut evaluator = crate::drift::DriftEvaluator::new(
            BlacklistConfig { patterns: vec![] },
            DriftWeights::default(),
        );

        while let Ok(ev) = rx.try_recv() {
            evaluator.process_event(&ev);
        }

        assert_eq!(evaluator.magnitude(), 0.0, "unknown categories should be ignored");
    }

    #[tokio::test]
    async fn observer_stop_is_idempotent() {
        let observer = MockObserver::new();
        observer.stop().await.unwrap();
        observer.stop().await.unwrap(); // double stop should not fail
    }

    // -----------------------------------------------------------------------
    // Linux-only tests for InotifyObserver and NetlinkObserver
    // -----------------------------------------------------------------------

    #[cfg(target_os = "linux")]
    mod linux_tests {
        use super::*;
        use std::path::PathBuf;
        use tokio::fs;
        use tokio::time::{timeout, Duration};

        #[tokio::test]
        async fn inotify_observer_detects_file_creation() {
            let dir = tempfile::tempdir().unwrap();
            let dir_path = dir.path().to_path_buf();

            let observer = super::super::InotifyObserver::new(vec![dir_path.clone()]);
            let (tx, mut rx) = mpsc::channel(32);

            observer.start(tx).await.unwrap();

            // Create a file in the watched directory.
            let test_file = dir_path.join("test.txt");
            fs::write(&test_file, b"hello").await.unwrap();

            // Wait for the event (with timeout).
            let event = timeout(Duration::from_secs(2), rx.recv())
                .await
                .expect("timed out waiting for inotify event")
                .expect("channel closed unexpectedly");

            assert_eq!(event.category, "file");
            assert!(
                event.path.contains("test.txt"),
                "path should contain filename: {}",
                event.path
            );

            observer.stop().await.unwrap();
        }

        #[tokio::test]
        async fn inotify_observer_stop_is_idempotent() {
            let observer = super::super::InotifyObserver::new(vec![PathBuf::from("/tmp")]);
            let (tx, _rx) = mpsc::channel(8);
            observer.start(tx).await.unwrap();

            observer.stop().await.unwrap();
            observer.stop().await.unwrap(); // should not fail
        }

        #[tokio::test]
        async fn inotify_observer_handles_dropped_receiver() {
            let dir = tempfile::tempdir().unwrap();
            let observer = super::super::InotifyObserver::new(vec![dir.path().to_path_buf()]);
            let (tx, rx) = mpsc::channel(1);
            drop(rx);

            // start should succeed even if receiver is already dropped
            let result = observer.start(tx).await;
            assert!(result.is_ok());
        }

        #[tokio::test]
        async fn netlink_observer_starts_and_stops() {
            let observer = super::super::NetlinkObserver::new();
            let (tx, _rx) = mpsc::channel(8);

            observer.start(tx).await.unwrap();
            observer.stop().await.unwrap();
        }

        #[tokio::test]
        async fn netlink_observer_stop_is_idempotent() {
            let observer = super::super::NetlinkObserver::new();
            let (tx, _rx) = mpsc::channel(8);
            observer.start(tx).await.unwrap();

            observer.stop().await.unwrap();
            observer.stop().await.unwrap();
        }

        #[tokio::test]
        async fn netlink_observer_default_trait() {
            let observer = super::super::NetlinkObserver::default();
            observer.stop().await.unwrap();
        }

        #[test]
        fn describe_inotify_mask_modify() {
            use nix::sys::inotify::AddWatchFlags;
            let desc = super::super::describe_inotify_mask(AddWatchFlags::IN_MODIFY);
            assert_eq!(desc, "modified");
        }

        #[test]
        fn describe_inotify_mask_combined() {
            use nix::sys::inotify::AddWatchFlags;
            let desc = super::super::describe_inotify_mask(
                AddWatchFlags::IN_CREATE | AddWatchFlags::IN_DELETE,
            );
            assert!(desc.contains("created"));
            assert!(desc.contains("deleted"));
        }

        #[test]
        fn describe_inotify_mask_unknown() {
            use nix::sys::inotify::AddWatchFlags;
            let desc = super::super::describe_inotify_mask(AddWatchFlags::IN_CLOSE_WRITE);
            assert!(desc.starts_with("inotify event 0x"), "got: {desc}");
        }
    }
}
