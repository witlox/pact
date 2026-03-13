//! State observer — detects changes to system state.
//!
//! Three mechanisms (Linux): eBPF probes, inotify file watches, netlink sockets.
//! On macOS: `MockObserver` for development/testing.
//! All observers emit `ObserverEvent` to the drift evaluator.

use chrono::{DateTime, Utc};
use tokio::sync::mpsc;

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
}
