//! Partition reconnect and conflict resolution (I3a, CR1-CR3).
//!
//! When a partitioned agent reconnects to the journal:
//! 1. Agent sends accumulated local changes to journal (CR1: local-first)
//! 2. Journal detects conflicts (local keys vs current state)
//! 3. If conflicts: agent pauses convergence for affected keys (CR2)
//! 4. Grace period timer starts (default: commit window duration)
//! 5. Admin resolves per key, or grace period expires → journal-wins (CR3)
//! 6. Overwritten local values logged for audit
//! 7. Agent resumes config subscription

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use tracing::{debug, info, warn};

/// A single conflicting config key with both local and journal values.
#[derive(Debug, Clone)]
pub struct ConflictEntry {
    /// Config key that conflicts.
    pub key: String,
    /// Value the agent has locally (from drift or emergency changes).
    pub local_value: Vec<u8>,
    /// Value the journal has (from other commits during partition).
    pub journal_value: Vec<u8>,
    /// When this conflict was detected.
    pub detected_at: DateTime<Utc>,
}

/// Resolution choice for a conflict.
#[derive(Debug, Clone, PartialEq)]
pub enum Resolution {
    /// Accept the local value (promote it to journal).
    AcceptLocal,
    /// Accept the journal value (discard local).
    AcceptJournal,
}

/// State of a pending conflict.
#[derive(Debug, Clone, PartialEq)]
pub enum ConflictState {
    /// Conflict detected, awaiting admin resolution.
    Pending,
    /// Admin resolved with a chosen value.
    Resolved(Resolution),
    /// Grace period expired — journal wins automatically.
    GraceExpired,
}

/// Tracks all pending conflicts for this node.
pub struct ConflictManager {
    /// Pending conflicts keyed by config key.
    conflicts: HashMap<String, (ConflictEntry, ConflictState)>,
    /// Grace period duration.
    grace_period: Duration,
    /// Keys paused from convergence (CR2).
    paused_keys: Vec<String>,
}

impl ConflictManager {
    pub fn new(grace_period_seconds: u32) -> Self {
        Self {
            conflicts: HashMap::new(),
            grace_period: Duration::seconds(i64::from(grace_period_seconds)),
            paused_keys: Vec::new(),
        }
    }

    /// Register a conflict manifest received from the journal.
    ///
    /// Pauses convergence for all conflicting keys (CR2).
    pub fn register_conflicts(&mut self, entries: Vec<ConflictEntry>) {
        info!(count = entries.len(), "Registering conflicts from journal");
        for entry in entries {
            let key = entry.key.clone();
            if !self.paused_keys.contains(&key) {
                self.paused_keys.push(key.clone());
            }
            self.conflicts.insert(key, (entry, ConflictState::Pending));
        }
    }

    /// Check if a given key is paused from convergence.
    pub fn is_paused(&self, key: &str) -> bool {
        self.paused_keys.contains(&key.to_string())
    }

    /// Get all pending (unresolved) conflicts.
    pub fn pending_conflicts(&self) -> Vec<&ConflictEntry> {
        self.conflicts
            .values()
            .filter(|(_, state)| *state == ConflictState::Pending)
            .map(|(entry, _)| entry)
            .collect()
    }

    /// Resolve a specific conflict by key.
    ///
    /// Returns the conflict entry and chosen resolution, or error if not found.
    pub fn resolve(&mut self, key: &str, resolution: Resolution) -> anyhow::Result<&ConflictEntry> {
        let (entry, state) = self
            .conflicts
            .get_mut(key)
            .ok_or_else(|| anyhow::anyhow!("no conflict for key: {key}"))?;

        if *state != ConflictState::Pending {
            anyhow::bail!("conflict for key {key} already resolved");
        }

        debug!(key, ?resolution, "Resolving conflict");
        *state = ConflictState::Resolved(resolution);

        // Un-pause the key
        self.paused_keys.retain(|k| k != key);
        Ok(entry)
    }

    /// Check for expired grace periods and apply journal-wins (CR3).
    ///
    /// Returns keys that were auto-resolved via journal-wins.
    pub fn check_grace_periods(&mut self) -> Vec<String> {
        let now = Utc::now();
        let mut expired_keys = Vec::new();

        for (key, (entry, state)) in &mut self.conflicts {
            if *state == ConflictState::Pending && now >= entry.detected_at + self.grace_period {
                warn!(key, "Grace period expired — journal-wins (CR3)");
                *state = ConflictState::GraceExpired;
                expired_keys.push(key.clone());
            }
        }

        // Un-pause expired keys
        for key in &expired_keys {
            self.paused_keys.retain(|k| k != key);
        }

        expired_keys
    }

    /// Check if all conflicts are resolved (no pending conflicts remain).
    pub fn all_resolved(&self) -> bool {
        self.conflicts.values().all(|(_, state)| *state != ConflictState::Pending)
    }

    /// Get the number of pending conflicts.
    pub fn pending_count(&self) -> usize {
        self.conflicts.values().filter(|(_, state)| *state == ConflictState::Pending).count()
    }

    /// Get all paused keys.
    pub fn paused_keys(&self) -> &[String] {
        &self.paused_keys
    }

    /// Clear all resolved conflicts (cleanup after reconnect completes).
    pub fn clear_resolved(&mut self) {
        self.conflicts.retain(|_, (_, state)| *state == ConflictState::Pending);
    }

    /// Get resolution results for logging/audit.
    pub fn resolution_log(&self) -> Vec<(String, ConflictState, Vec<u8>, Vec<u8>)> {
        self.conflicts
            .iter()
            .filter(|(_, (_, state))| *state != ConflictState::Pending)
            .map(|(key, (entry, state))| {
                (key.clone(), state.clone(), entry.local_value.clone(), entry.journal_value.clone())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conflict(key: &str) -> ConflictEntry {
        ConflictEntry {
            key: key.into(),
            local_value: format!("local-{key}").into_bytes(),
            journal_value: format!("journal-{key}").into_bytes(),
            detected_at: Utc::now(),
        }
    }

    /// Create a conflict that was detected `seconds_ago` seconds in the past.
    fn past_conflict(key: &str, seconds_ago: i64) -> ConflictEntry {
        ConflictEntry {
            key: key.into(),
            local_value: format!("local-{key}").into_bytes(),
            journal_value: format!("journal-{key}").into_bytes(),
            detected_at: Utc::now() - chrono::Duration::seconds(seconds_ago),
        }
    }

    #[test]
    fn register_and_query_conflicts() {
        let mut mgr = ConflictManager::new(900);
        mgr.register_conflicts(vec![conflict("sysctl.vm.swappiness"), conflict("mount./data")]);

        assert_eq!(mgr.pending_count(), 2);
        assert!(mgr.is_paused("sysctl.vm.swappiness"));
        assert!(mgr.is_paused("mount./data"));
        assert!(!mgr.is_paused("unrelated.key"));
        assert!(!mgr.all_resolved());
    }

    #[test]
    fn resolve_accept_local() {
        let mut mgr = ConflictManager::new(900);
        mgr.register_conflicts(vec![conflict("sysctl.vm.swappiness")]);

        let entry = mgr.resolve("sysctl.vm.swappiness", Resolution::AcceptLocal).unwrap();
        assert_eq!(entry.key, "sysctl.vm.swappiness");
        assert!(!mgr.is_paused("sysctl.vm.swappiness"));
        assert!(mgr.all_resolved());
    }

    #[test]
    fn resolve_accept_journal() {
        let mut mgr = ConflictManager::new(900);
        mgr.register_conflicts(vec![conflict("mount./data")]);

        mgr.resolve("mount./data", Resolution::AcceptJournal).unwrap();
        assert_eq!(mgr.pending_count(), 0);
    }

    #[test]
    fn resolve_nonexistent_key_errors() {
        let mut mgr = ConflictManager::new(900);
        let result = mgr.resolve("nonexistent", Resolution::AcceptLocal);
        assert!(result.is_err());
    }

    #[test]
    fn resolve_already_resolved_errors() {
        let mut mgr = ConflictManager::new(900);
        mgr.register_conflicts(vec![conflict("key1")]);
        mgr.resolve("key1", Resolution::AcceptLocal).unwrap();

        let result = mgr.resolve("key1", Resolution::AcceptJournal);
        assert!(result.is_err());
    }

    #[test]
    fn grace_period_expiry_journal_wins() {
        // Grace period is 60 seconds, but conflicts were detected 120 seconds ago
        let mut mgr = ConflictManager::new(60);
        mgr.register_conflicts(vec![past_conflict("key1", 120), past_conflict("key2", 120)]);

        let expired = mgr.check_grace_periods();
        assert_eq!(expired.len(), 2);
        assert!(expired.contains(&"key1".to_string()));
        assert!(expired.contains(&"key2".to_string()));

        // Keys should be un-paused
        assert!(!mgr.is_paused("key1"));
        assert!(!mgr.is_paused("key2"));
        assert!(mgr.all_resolved());
    }

    #[test]
    fn grace_period_not_expired_yet() {
        // Grace period is 3600 seconds, conflict was detected just now
        let mut mgr = ConflictManager::new(3600);
        mgr.register_conflicts(vec![conflict("key1")]);

        let expired = mgr.check_grace_periods();
        assert!(expired.is_empty());
        assert!(mgr.is_paused("key1"));
        assert_eq!(mgr.pending_count(), 1);
    }

    #[test]
    fn grace_period_boundary_old_conflict_expires_new_does_not() {
        // Grace period = 300 seconds
        let mut mgr = ConflictManager::new(300);
        // key1 detected 600 seconds ago → expired
        // key2 detected 10 seconds ago → still within grace
        mgr.register_conflicts(vec![past_conflict("key1", 600), past_conflict("key2", 10)]);

        let expired = mgr.check_grace_periods();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0], "key1");

        assert!(!mgr.is_paused("key1")); // expired, un-paused
        assert!(mgr.is_paused("key2")); // still pending
        assert_eq!(mgr.pending_count(), 1);
    }

    #[test]
    fn partial_resolution_mixed_with_grace_expiry() {
        // Grace period = 30 seconds, conflicts detected 60 seconds ago
        let mut mgr = ConflictManager::new(30);
        mgr.register_conflicts(vec![past_conflict("key1", 60), past_conflict("key2", 60)]);

        // Resolve key1 manually before grace period check
        mgr.resolve("key1", Resolution::AcceptLocal).unwrap();

        // key2 should expire via grace period
        let expired = mgr.check_grace_periods();
        assert_eq!(expired, vec!["key2"]);
        assert!(mgr.all_resolved());
    }

    #[test]
    fn clear_resolved_keeps_pending() {
        let mut mgr = ConflictManager::new(3600);
        mgr.register_conflicts(vec![conflict("resolved"), conflict("pending")]);
        mgr.resolve("resolved", Resolution::AcceptLocal).unwrap();

        mgr.clear_resolved();
        assert_eq!(mgr.pending_count(), 1);
        assert!(mgr.is_paused("pending"));
    }

    #[test]
    fn resolution_log_contains_resolved() {
        let mut mgr = ConflictManager::new(900);
        mgr.register_conflicts(vec![conflict("key1"), conflict("key2")]);
        mgr.resolve("key1", Resolution::AcceptLocal).unwrap();

        let log = mgr.resolution_log();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].0, "key1");
        assert_eq!(log[0].1, ConflictState::Resolved(Resolution::AcceptLocal));
    }

    #[test]
    fn empty_manager() {
        let mgr = ConflictManager::new(900);
        assert_eq!(mgr.pending_count(), 0);
        assert!(mgr.all_resolved());
        assert!(mgr.paused_keys().is_empty());
    }
}
