//! State observer — detects changes to system state.
//!
//! Three mechanisms (Linux): eBPF probes, inotify file watches, netlink sockets.
//! On macOS: `MockObserver` for development/testing.
//! All observers emit `ObserverEvent` to the drift evaluator.

use chrono::{DateTime, Utc};
use tokio::sync::mpsc;

#[cfg(target_os = "linux")]
use std::os::fd::AsFd;
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

        // Wrap the inotify fd for async I/O. We use the raw fd via AsFd
        // and move the Inotify into the task to keep it alive.
        let raw_fd = std::os::fd::OwnedFd::from(inotify.as_fd().try_clone_to_owned()?);
        let async_fd = tokio::io::unix::AsyncFd::new(raw_fd)?;

        tokio::spawn(async move {
            // Keep inotify alive in the task (it owns the watch descriptors)
            let _inotify_guard = &inotify;

            while running.load(Ordering::SeqCst) {
                // Wait until the inotify fd is readable.
                let mut guard = match async_fd.readable().await {
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

        bind(sock.as_fd(), &addr)?;

        self.running.store(true, Ordering::SeqCst);
        let running = Arc::clone(&self.running);

        // Clone the fd for async wrapping; original `sock` moves into task
        let async_fd_owned = sock.as_fd().try_clone_to_owned()?;
        let async_fd = tokio::io::unix::AsyncFd::new(async_fd_owned)?;

        tokio::spawn(async move {
            // Keep the socket alive for the duration of the task
            let _sock_guard = &sock;

            let mut buf = [0u8; 4096];

            while running.load(Ordering::SeqCst) {
                let mut guard = match async_fd.readable().await {
                    Ok(g) => g,
                    Err(_) => break,
                };

                match nix::unistd::read(sock.as_fd(), &mut buf) {
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

// ---------------------------------------------------------------------------
// EbpfObserver — eBPF probe loader (Linux only, feature = "ebpf")
// ---------------------------------------------------------------------------

/// Loads compiled eBPF programs (`.o` files) and attaches them as tracepoints
/// to detect system state changes (mounts, hostname changes, sysctl writes).
///
/// Program-to-tracepoint mapping is by filename convention:
/// - `mount.o`       → `syscalls/sys_enter_mount`
/// - `sethostname.o` → `syscalls/sys_enter_sethostname`
/// - `sysctl.o`      → `syscalls/sys_enter_sysctl`
///
/// Events are read from perf event arrays and forwarded as `ObserverEvent`.
#[cfg(feature = "ebpf")]
pub struct EbpfObserver {
    programs_dir: PathBuf,
    running: Arc<AtomicBool>,
}

#[cfg(feature = "ebpf")]
impl EbpfObserver {
    /// Create a new eBPF observer that loads programs from the given directory.
    pub fn new(programs_dir: PathBuf) -> Self {
        Self {
            programs_dir,
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Map a BPF object filename to the tracepoint it should attach to.
    fn tracepoint_for_file(filename: &str) -> Option<(&'static str, &'static str)> {
        match filename {
            "mount.o" => Some(("syscalls", "sys_enter_mount")),
            "sethostname.o" => Some(("syscalls", "sys_enter_sethostname")),
            "sysctl.o" => Some(("syscalls", "sys_enter_sysctl")),
            _ => None,
        }
    }
}

#[cfg(feature = "ebpf")]
#[async_trait::async_trait]
impl Observer for EbpfObserver {
    async fn start(&self, tx: mpsc::Sender<ObserverEvent>) -> anyhow::Result<()> {
        use aya::programs::TracePoint;
        use aya::Ebpf;

        self.running.store(true, Ordering::SeqCst);
        let running = Arc::clone(&self.running);

        // Scan the programs directory for .o files.
        let entries = match std::fs::read_dir(&self.programs_dir) {
            Ok(entries) => entries,
            Err(e) => {
                tracing::error!(
                    dir = %self.programs_dir.display(),
                    error = %e,
                    "EbpfObserver: failed to read programs directory"
                );
                return Err(e.into());
            }
        };

        let mut loaded_count = 0u32;

        for entry in entries.flatten() {
            let path = entry.path();
            let filename = match path.file_name().and_then(|n| n.to_str()) {
                Some(name) if name.ends_with(".o") => name.to_string(),
                _ => continue,
            };

            let (category, tracepoint_name) = match Self::tracepoint_for_file(&filename) {
                Some(tp) => tp,
                None => {
                    tracing::warn!(
                        file = %filename,
                        "EbpfObserver: no tracepoint mapping for BPF object, skipping"
                    );
                    continue;
                }
            };

            // Load the BPF program from the .o file.
            let mut bpf = match Ebpf::load_file(&path) {
                Ok(bpf) => bpf,
                Err(e) => {
                    tracing::warn!(
                        file = %filename,
                        error = %e,
                        "EbpfObserver: failed to load BPF program, skipping"
                    );
                    continue;
                }
            };

            // Find the tracepoint program inside the loaded BPF object.
            // Convention: the BPF program function name matches the tracepoint name.
            let prog_name = tracepoint_name.to_string();
            let program = match bpf.program_mut(&prog_name) {
                Some(prog) => prog,
                None => {
                    tracing::warn!(
                        file = %filename,
                        program = %prog_name,
                        "EbpfObserver: program not found in BPF object, skipping"
                    );
                    continue;
                }
            };

            // Load and attach the tracepoint.
            let tp: &mut TracePoint = match program.try_into() {
                Ok(tp) => tp,
                Err(e) => {
                    tracing::warn!(
                        file = %filename,
                        error = %e,
                        "EbpfObserver: program is not a tracepoint, skipping"
                    );
                    continue;
                }
            };

            if let Err(e) = tp.load() {
                tracing::warn!(
                    file = %filename,
                    error = %e,
                    "EbpfObserver: failed to load tracepoint program, skipping"
                );
                continue;
            }

            if let Err(e) = tp.attach(category, tracepoint_name) {
                tracing::warn!(
                    file = %filename,
                    tracepoint = %tracepoint_name,
                    error = %e,
                    "EbpfObserver: failed to attach tracepoint, skipping"
                );
                continue;
            }

            tracing::info!(
                file = %filename,
                tracepoint = format!("{category}/{tracepoint_name}"),
                "EbpfObserver: loaded and attached BPF program"
            );
            loaded_count += 1;
        }

        tracing::info!(
            count = loaded_count,
            "EbpfObserver: finished loading BPF programs"
        );

        // Spawn a task that reads perf events (skeleton — real implementation
        // would read from perf event arrays defined in the BPF programs).
        tokio::spawn(async move {
            while running.load(Ordering::SeqCst) {
                // In a full implementation, this would poll aya's
                // AsyncPerfEventArray for events from the BPF programs
                // and forward them as ObserverEvents via tx.
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                // Placeholder: the actual event reading loop would do:
                // let events = perf_array.read_events(...);
                // for event in events { tx.send(ObserverEvent { ... }).await; }
                let _ = &tx; // suppress unused warning
            }
        });

        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// EbpfObserver stub — when the "ebpf" feature is not compiled in
// ---------------------------------------------------------------------------

/// Stub eBPF observer when the `ebpf` feature is not enabled.
///
/// Returns an error on `start()` indicating eBPF support is not compiled in.
#[cfg(not(feature = "ebpf"))]
pub struct EbpfObserver {
    programs_dir: std::path::PathBuf,
}

#[cfg(not(feature = "ebpf"))]
impl EbpfObserver {
    /// Create a new (stub) eBPF observer.
    pub fn new(programs_dir: std::path::PathBuf) -> Self {
        Self { programs_dir }
    }
}

#[cfg(not(feature = "ebpf"))]
#[async_trait::async_trait]
impl Observer for EbpfObserver {
    async fn start(&self, _tx: mpsc::Sender<ObserverEvent>) -> anyhow::Result<()> {
        Err(anyhow::anyhow!(
            "eBPF support is not compiled in (enable the 'ebpf' feature). programs_dir={:?}",
            self.programs_dir
        ))
    }

    async fn stop(&self) -> anyhow::Result<()> {
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
    // EbpfObserver tests (stub — no ebpf feature)
    // -----------------------------------------------------------------------

    #[cfg(not(feature = "ebpf"))]
    mod ebpf_stub_tests {
        use super::*;
        use std::path::PathBuf;

        #[test]
        fn ebpf_observer_stub_constructs() {
            let observer = EbpfObserver::new(PathBuf::from("/usr/lib/pact/bpf"));
            assert_eq!(observer.programs_dir, PathBuf::from("/usr/lib/pact/bpf"));
        }

        #[tokio::test]
        async fn ebpf_observer_stub_start_returns_error() {
            let observer = EbpfObserver::new(PathBuf::from("/nonexistent"));
            let (tx, _rx) = mpsc::channel(8);

            let result = observer.start(tx).await;
            assert!(result.is_err());
            let err_msg = result.unwrap_err().to_string();
            assert!(
                err_msg.contains("eBPF support is not compiled in"),
                "unexpected error: {err_msg}"
            );
        }

        #[tokio::test]
        async fn ebpf_observer_stub_stop_is_ok() {
            let observer = EbpfObserver::new(PathBuf::from("/nonexistent"));
            assert!(observer.stop().await.is_ok());
        }
    }

    #[cfg(feature = "ebpf")]
    mod ebpf_tests {
        use super::*;
        use std::path::PathBuf;

        #[test]
        fn ebpf_observer_constructs() {
            let observer = EbpfObserver::new(PathBuf::from("/usr/lib/pact/bpf"));
            assert_eq!(observer.programs_dir, PathBuf::from("/usr/lib/pact/bpf"));
            assert!(!observer.running.load(std::sync::atomic::Ordering::SeqCst));
        }

        #[test]
        fn ebpf_tracepoint_mapping() {
            assert_eq!(
                EbpfObserver::tracepoint_for_file("mount.o"),
                Some(("syscalls", "sys_enter_mount"))
            );
            assert_eq!(
                EbpfObserver::tracepoint_for_file("sethostname.o"),
                Some(("syscalls", "sys_enter_sethostname"))
            );
            assert_eq!(
                EbpfObserver::tracepoint_for_file("sysctl.o"),
                Some(("syscalls", "sys_enter_sysctl"))
            );
            assert_eq!(EbpfObserver::tracepoint_for_file("unknown.o"), None);
            assert_eq!(EbpfObserver::tracepoint_for_file("mount.c"), None);
        }

        #[tokio::test]
        async fn ebpf_observer_start_with_empty_dir() {
            let dir = tempfile::tempdir().unwrap();
            let observer = EbpfObserver::new(dir.path().to_path_buf());
            let (tx, _rx) = mpsc::channel(8);

            // Empty directory — should succeed with 0 programs loaded.
            let result = observer.start(tx).await;
            assert!(result.is_ok());
            observer.stop().await.unwrap();
        }

        #[tokio::test]
        async fn ebpf_observer_start_with_nonexistent_dir() {
            let observer = EbpfObserver::new(PathBuf::from("/nonexistent/bpf/programs"));
            let (tx, _rx) = mpsc::channel(8);

            let result = observer.start(tx).await;
            assert!(result.is_err());
        }

        #[tokio::test]
        async fn ebpf_observer_stop_is_idempotent() {
            let dir = tempfile::tempdir().unwrap();
            let observer = EbpfObserver::new(dir.path().to_path_buf());
            observer.stop().await.unwrap();
            observer.stop().await.unwrap();
        }
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
