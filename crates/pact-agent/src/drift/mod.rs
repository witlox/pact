//! Drift evaluator — compares actual vs declared state.
//!
//! Computes a `DriftVector` across 7 dimensions and applies
//! per-vCluster weights from `VClusterPolicy`.
//! Blacklist filtering excludes known-noisy paths (ADR-002).

use pact_common::config::BlacklistConfig;
use pact_common::types::{DriftVector, DriftWeights};

use crate::observer::ObserverEvent;

/// Evaluates drift from observer events and produces a drift vector.
pub struct DriftEvaluator {
    blacklist: BlacklistConfig,
    weights: DriftWeights,
    /// Current accumulated drift vector.
    current_drift: DriftVector,
}

impl DriftEvaluator {
    pub fn new(blacklist: BlacklistConfig, weights: DriftWeights) -> Self {
        Self { blacklist, weights, current_drift: DriftVector::default() }
    }

    /// Process an observer event and update the drift vector.
    pub fn process_event(&mut self, event: &ObserverEvent) -> &DriftVector {
        // Skip blacklisted paths
        if self.is_blacklisted(&event.path) {
            return &self.current_drift;
        }

        match event.category.as_str() {
            "mount" => self.current_drift.mounts += 1.0,
            "file" => self.current_drift.files += 1.0,
            "network" => self.current_drift.network += 1.0,
            "service" => self.current_drift.services += 1.0,
            "kernel" => self.current_drift.kernel += 1.0,
            "package" => self.current_drift.packages += 1.0,
            "gpu" => self.current_drift.gpu += 1.0,
            _ => {} // Unknown category — ignore
        }

        &self.current_drift
    }

    /// Get the current drift magnitude (weighted).
    pub fn magnitude(&self) -> f64 {
        self.current_drift.magnitude(&self.weights)
    }

    /// Get the current drift vector.
    pub fn drift_vector(&self) -> &DriftVector {
        &self.current_drift
    }

    /// Reset drift (e.g., after commit).
    pub fn reset(&mut self) {
        self.current_drift = DriftVector::default();
    }

    /// Check if a path matches any blacklist pattern.
    fn is_blacklisted(&self, path: &str) -> bool {
        for pattern in &self.blacklist.patterns {
            if matches_glob(pattern, path) {
                return true;
            }
        }
        false
    }

    /// Update weights (e.g., from new VClusterPolicy).
    pub fn set_weights(&mut self, weights: DriftWeights) {
        self.weights = weights;
    }
}

/// Simple glob matching supporting `**` (match anything) and `*` (single segment).
fn matches_glob(pattern: &str, path: &str) -> bool {
    if pattern.ends_with("/**") {
        let prefix = &pattern[..pattern.len() - 3];
        return path.starts_with(prefix);
    }
    if pattern.ends_with("/*") {
        let prefix = &pattern[..pattern.len() - 2];
        if !path.starts_with(prefix) {
            return false;
        }
        let rest = &path[prefix.len()..];
        // Single segment: no more slashes
        return !rest[1..].contains('/');
    }
    pattern == path
}

#[cfg(test)]
mod tests {
    use super::*;
    use pact_common::config::BlacklistConfig;

    fn default_evaluator() -> DriftEvaluator {
        DriftEvaluator::new(BlacklistConfig::default(), DriftWeights::default())
    }

    fn event(category: &str, path: &str) -> ObserverEvent {
        ObserverEvent {
            category: category.into(),
            path: path.into(),
            detail: String::new(),
            timestamp: chrono::Utc::now(),
        }
    }

    #[test]
    fn process_kernel_event_increments_drift() {
        let mut eval = default_evaluator();
        eval.process_event(&event("kernel", "/proc/sys/kernel/shmmax"));
        // /proc/** is blacklisted by default
        assert_eq!(eval.drift_vector().kernel, 0.0);

        // Non-blacklisted kernel event
        eval.process_event(&event("kernel", "/etc/sysctl.conf"));
        assert_eq!(eval.drift_vector().kernel, 1.0);
    }

    #[test]
    fn blacklist_filters_default_paths() {
        let mut eval = default_evaluator();
        eval.process_event(&event("file", "/tmp/something"));
        eval.process_event(&event("file", "/var/log/syslog"));
        eval.process_event(&event("file", "/proc/cpuinfo"));
        eval.process_event(&event("file", "/sys/class/net"));
        eval.process_event(&event("file", "/dev/null"));
        eval.process_event(&event("file", "/run/user/1000/test"));
        assert_eq!(eval.drift_vector().files, 0.0);
    }

    #[test]
    fn non_blacklisted_events_counted() {
        let mut eval = default_evaluator();
        eval.process_event(&event("mount", "/mnt/data"));
        eval.process_event(&event("file", "/etc/pact/config.toml"));
        eval.process_event(&event("network", "eth0"));
        eval.process_event(&event("service", "lattice-node-agent"));
        assert_eq!(eval.drift_vector().mounts, 1.0);
        assert_eq!(eval.drift_vector().files, 1.0);
        assert_eq!(eval.drift_vector().network, 1.0);
        assert_eq!(eval.drift_vector().services, 1.0);
    }

    #[test]
    fn magnitude_is_weighted() {
        let mut eval = default_evaluator();
        eval.process_event(&event("kernel", "/etc/sysctl.conf"));
        eval.process_event(&event("kernel", "/etc/sysctl.d/99-pact.conf"));
        let mag = eval.magnitude();
        assert!(mag > 0.0);
    }

    #[test]
    fn reset_clears_drift() {
        let mut eval = default_evaluator();
        eval.process_event(&event("kernel", "/etc/sysctl.conf"));
        assert!(eval.magnitude() > 0.0);
        eval.reset();
        assert_eq!(eval.magnitude(), 0.0);
    }

    #[test]
    fn glob_matching() {
        assert!(matches_glob("/tmp/**", "/tmp/foo"));
        assert!(matches_glob("/tmp/**", "/tmp/foo/bar"));
        assert!(!matches_glob("/tmp/**", "/var/tmp/foo"));
        assert!(matches_glob("/var/log/**", "/var/log/syslog"));
        assert_eq!(matches_glob("/etc/foo", "/etc/foo"), true);
        assert_eq!(matches_glob("/etc/foo", "/etc/bar"), false);
    }

    #[test]
    fn custom_blacklist() {
        let mut eval = DriftEvaluator::new(
            BlacklistConfig { patterns: vec!["/scratch/**".into()] },
            DriftWeights::default(),
        );
        eval.process_event(&event("file", "/scratch/job123/output"));
        assert_eq!(eval.drift_vector().files, 0.0);

        eval.process_event(&event("file", "/etc/config"));
        assert_eq!(eval.drift_vector().files, 1.0);
    }
}
