//! Emergency mode — extended change window with full audit trail.
//!
//! `pact emergency --reason "..."` enters emergency mode:
//! - Extended commit window (4h default)
//! - Rollback suspended
//! - Does NOT expand shell whitelist (ADR-004)
//! - Must end with explicit commit or rollback
//! - Stale emergency triggers alert
//! - Another admin can force-end

use chrono::{DateTime, Duration, Utc};
use pact_common::types::Identity;

/// Emergency mode state.
#[derive(Debug, Clone)]
pub enum EmergencyState {
    /// Normal operation.
    Inactive,
    /// Emergency mode active.
    Active {
        started_at: DateTime<Utc>,
        started_by: Identity,
        reason: String,
        expires_at: DateTime<Utc>,
    },
}

/// Manages emergency mode lifecycle.
pub struct EmergencyManager {
    state: EmergencyState,
    default_window_seconds: u32,
}

impl EmergencyManager {
    pub fn new(default_window_seconds: u32) -> Self {
        Self { state: EmergencyState::Inactive, default_window_seconds }
    }

    /// Enter emergency mode. Returns error if already active.
    pub fn start(&mut self, actor: Identity, reason: String) -> anyhow::Result<()> {
        if self.is_active() {
            anyhow::bail!("emergency mode already active");
        }

        let now = Utc::now();
        self.state = EmergencyState::Active {
            started_at: now,
            started_by: actor,
            reason,
            expires_at: now + Duration::seconds(i64::from(self.default_window_seconds)),
        };
        Ok(())
    }

    /// End emergency mode. `force` allows another admin to end it.
    pub fn end(&mut self, actor: &Identity, force: bool) -> anyhow::Result<()> {
        match &self.state {
            EmergencyState::Inactive => anyhow::bail!("not in emergency mode"),
            EmergencyState::Active { started_by, .. } => {
                if !force && started_by.principal != actor.principal {
                    anyhow::bail!(
                        "only {} or force-end can close this emergency",
                        started_by.principal
                    );
                }
                self.state = EmergencyState::Inactive;
                Ok(())
            }
        }
    }

    /// Check if emergency mode is active.
    pub fn is_active(&self) -> bool {
        matches!(self.state, EmergencyState::Active { .. })
    }

    /// Check if emergency has gone stale (past its expiry without resolution).
    pub fn is_stale(&self) -> bool {
        match &self.state {
            EmergencyState::Active { expires_at, .. } => Utc::now() >= *expires_at,
            _ => false,
        }
    }

    /// Get the current state.
    pub fn state(&self) -> &EmergencyState {
        &self.state
    }

    /// Get the reason (if active).
    pub fn reason(&self) -> Option<&str> {
        match &self.state {
            EmergencyState::Active { reason, .. } => Some(reason),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pact_common::types::PrincipalType;

    fn admin(name: &str) -> Identity {
        Identity {
            principal: name.into(),
            principal_type: PrincipalType::Human,
            role: "pact-platform-admin".into(),
        }
    }

    #[test]
    fn start_and_end_emergency() {
        let mut mgr = EmergencyManager::new(14400);
        assert!(!mgr.is_active());

        mgr.start(admin("alice@example.com"), "network reconfiguration".into()).unwrap();
        assert!(mgr.is_active());
        assert_eq!(mgr.reason(), Some("network reconfiguration"));

        mgr.end(&admin("alice@example.com"), false).unwrap();
        assert!(!mgr.is_active());
    }

    #[test]
    fn cannot_start_twice() {
        let mut mgr = EmergencyManager::new(14400);
        mgr.start(admin("alice@example.com"), "reason 1".into()).unwrap();
        let result = mgr.start(admin("bob@example.com"), "reason 2".into());
        assert!(result.is_err());
    }

    #[test]
    fn different_admin_cannot_end_without_force() {
        let mut mgr = EmergencyManager::new(14400);
        mgr.start(admin("alice@example.com"), "reason".into()).unwrap();

        let result = mgr.end(&admin("bob@example.com"), false);
        assert!(result.is_err());
        assert!(mgr.is_active());
    }

    #[test]
    fn force_end_by_different_admin() {
        let mut mgr = EmergencyManager::new(14400);
        mgr.start(admin("alice@example.com"), "reason".into()).unwrap();

        mgr.end(&admin("bob@example.com"), true).unwrap();
        assert!(!mgr.is_active());
    }

    #[test]
    fn stale_detection() {
        let mut mgr = EmergencyManager::new(0); // 0 second window = immediately stale
        mgr.start(admin("alice@example.com"), "test".into()).unwrap();
        assert!(mgr.is_stale());
    }

    #[test]
    fn not_stale_when_inactive() {
        let mgr = EmergencyManager::new(14400);
        assert!(!mgr.is_stale());
    }

    #[test]
    fn end_when_not_active_errors() {
        let mut mgr = EmergencyManager::new(14400);
        let result = mgr.end(&admin("alice@example.com"), false);
        assert!(result.is_err());
    }
}
