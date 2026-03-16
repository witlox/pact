//! Commit window manager — optimistic concurrency with drift-based deadlines.
//!
//! Formula: `window_seconds = base_window / (1 + drift_magnitude * sensitivity)`
//! Higher drift = shorter window = more urgent to commit or rollback.
//!
//! States:
//! - Idle: no active drift
//! - Open: drift detected, timer running
//! - Expired: window closed, rollback pending (unless emergency mode)

use chrono::{DateTime, Duration, Utc};
use pact_common::config::CommitWindowConfig;

/// Commit window state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindowState {
    /// No drift — no active window.
    Idle,
    /// Window open — drift detected, deadline approaching.
    Open { opened_at: DateTime<Utc>, deadline: DateTime<Utc> },
    /// Window expired — rollback needed (unless emergency mode).
    Expired { opened_at: DateTime<Utc>, expired_at: DateTime<Utc> },
}

/// Manages commit windows based on drift magnitude.
pub struct CommitWindowManager {
    config: CommitWindowConfig,
    state: WindowState,
    emergency: bool,
}

impl CommitWindowManager {
    pub fn new(config: CommitWindowConfig) -> Self {
        Self { config, state: WindowState::Idle, emergency: false }
    }

    /// Update config from a policy change (live reconfiguration).
    pub fn update_config(
        &mut self,
        base_window_seconds: u32,
        drift_sensitivity: f64,
        emergency_window_seconds: u32,
    ) {
        self.config.base_window_seconds = base_window_seconds;
        self.config.drift_sensitivity = drift_sensitivity;
        self.config.emergency_window_seconds = emergency_window_seconds;
    }

    /// Calculate window duration based on drift magnitude.
    ///
    /// `window = base_window / (1 + magnitude * sensitivity)`
    pub fn calculate_window_seconds(&self, drift_magnitude: f64) -> u32 {
        let denominator = drift_magnitude.mul_add(self.config.drift_sensitivity, 1.0);
        let window = f64::from(self.config.base_window_seconds) / denominator;
        // Clamp to at least 60 seconds
        window.max(60.0) as u32
    }

    /// Open a commit window for the given drift magnitude.
    /// If already open, updates the deadline (drift may have changed).
    pub fn open(&mut self, drift_magnitude: f64) {
        let window_secs = if self.emergency {
            self.config.emergency_window_seconds
        } else {
            self.calculate_window_seconds(drift_magnitude)
        };

        let now = Utc::now();
        let deadline = now + Duration::seconds(i64::from(window_secs));

        self.state = WindowState::Open {
            opened_at: match &self.state {
                WindowState::Open { opened_at, .. } => *opened_at,
                _ => now,
            },
            deadline,
        };
    }

    /// Check if the window has expired. Updates state if so.
    pub fn check_expired(&mut self) -> bool {
        match &self.state {
            WindowState::Open { opened_at, deadline } if Utc::now() >= *deadline => {
                if self.emergency {
                    false // Emergency mode: never expires
                } else {
                    self.state =
                        WindowState::Expired { opened_at: *opened_at, expired_at: *deadline };
                    true
                }
            }
            WindowState::Expired { .. } => true,
            _ => false,
        }
    }

    /// Commit: acknowledge drift, close window.
    pub fn commit(&mut self) {
        self.state = WindowState::Idle;
    }

    /// Rollback: revert drift, close window.
    pub fn rollback(&mut self) {
        self.state = WindowState::Idle;
    }

    /// Rollback with active consumer check (A5).
    ///
    /// Rejects rollback if there are active consumers using the current config.
    /// Use `active_consumers: 0` when consumer tracking is not yet available.
    pub fn rollback_with_check(&mut self, active_consumers: usize) -> Result<(), String> {
        if active_consumers > 0 {
            return Err(format!("{active_consumers} active consumers — rollback blocked (A5)"));
        }
        self.state = WindowState::Idle;
        Ok(())
    }

    /// Extend the window by the given number of seconds.
    pub fn extend(&mut self, additional_seconds: u32) {
        if let WindowState::Open { opened_at, deadline } = &self.state {
            self.state = WindowState::Open {
                opened_at: *opened_at,
                deadline: *deadline + Duration::seconds(i64::from(additional_seconds)),
            };
        }
    }

    /// Enter emergency mode — extends window, suspends expiry.
    pub fn enter_emergency(&mut self) {
        self.emergency = true;
        // If window is open, extend to emergency duration
        if matches!(self.state, WindowState::Open { .. }) {
            let now = Utc::now();
            self.state = WindowState::Open {
                opened_at: now,
                deadline: now + Duration::seconds(i64::from(self.config.emergency_window_seconds)),
            };
        }
    }

    /// Exit emergency mode.
    pub fn exit_emergency(&mut self) {
        self.emergency = false;
    }

    /// Get the current config.
    pub fn config(&self) -> &CommitWindowConfig {
        &self.config
    }

    /// Get current state.
    pub fn state(&self) -> &WindowState {
        &self.state
    }

    /// Is emergency mode active?
    pub fn is_emergency(&self) -> bool {
        self.emergency
    }

    /// Seconds remaining until deadline (0 if expired or idle).
    pub fn seconds_remaining(&self) -> u32 {
        match &self.state {
            WindowState::Open { deadline, .. } => {
                let remaining = (*deadline - Utc::now()).num_seconds();
                remaining.max(0) as u32
            }
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> CommitWindowConfig {
        CommitWindowConfig::default() // base=900, sensitivity=2.0, emergency=14400
    }

    #[test]
    fn window_calculation_no_drift() {
        let mgr = CommitWindowManager::new(default_config());
        // 900 / (1 + 0 * 2) = 900
        assert_eq!(mgr.calculate_window_seconds(0.0), 900);
    }

    #[test]
    fn window_calculation_moderate_drift() {
        let mgr = CommitWindowManager::new(default_config());
        // 900 / (1 + 1.0 * 2.0) = 900 / 3 = 300
        assert_eq!(mgr.calculate_window_seconds(1.0), 300);
    }

    #[test]
    fn window_calculation_high_drift() {
        let mgr = CommitWindowManager::new(default_config());
        // 900 / (1 + 5.0 * 2.0) = 900 / 11 ≈ 81
        assert_eq!(mgr.calculate_window_seconds(5.0), 81);
    }

    #[test]
    fn window_clamps_at_60_seconds() {
        let mgr = CommitWindowManager::new(default_config());
        // Very high drift should clamp to 60
        assert_eq!(mgr.calculate_window_seconds(100.0), 60);
    }

    #[test]
    fn open_and_commit_cycle() {
        let mut mgr = CommitWindowManager::new(default_config());
        assert!(matches!(mgr.state(), WindowState::Idle));

        mgr.open(1.0);
        assert!(matches!(mgr.state(), WindowState::Open { .. }));
        assert!(mgr.seconds_remaining() > 0);

        mgr.commit();
        assert!(matches!(mgr.state(), WindowState::Idle));
    }

    #[test]
    fn open_and_rollback_cycle() {
        let mut mgr = CommitWindowManager::new(default_config());
        mgr.open(1.0);
        mgr.rollback();
        assert!(matches!(mgr.state(), WindowState::Idle));
    }

    #[test]
    fn extend_adds_time() {
        let mut mgr = CommitWindowManager::new(default_config());
        mgr.open(1.0);
        let before = mgr.seconds_remaining();
        mgr.extend(300);
        let after = mgr.seconds_remaining();
        // Should have gained ~300 seconds (within tolerance for test timing)
        assert!(after > before + 250);
    }

    #[test]
    fn emergency_mode_uses_extended_window() {
        let mut mgr = CommitWindowManager::new(default_config());
        mgr.enter_emergency();
        mgr.open(1.0);
        // Emergency window is 14400 seconds (4 hours)
        assert!(mgr.seconds_remaining() > 14000);
    }

    #[test]
    fn emergency_mode_prevents_expiry() {
        let mut mgr = CommitWindowManager::new(CommitWindowConfig {
            base_window_seconds: 1,
            drift_sensitivity: 0.0,
            emergency_window_seconds: 14400,
        });
        mgr.enter_emergency();
        mgr.open(0.0);
        // Even with tiny base window, emergency prevents expiry
        assert!(!mgr.check_expired());
    }

    #[test]
    fn rollback_with_check_blocks_on_active_consumers() {
        let mut mgr = CommitWindowManager::new(default_config());
        mgr.open(1.0);
        let err = mgr.rollback_with_check(3).unwrap_err();
        assert!(err.contains("3 active consumers"));
        // State should still be Open (rollback was blocked)
        assert!(matches!(mgr.state(), WindowState::Open { .. }));
    }

    #[test]
    fn rollback_with_check_succeeds_when_no_consumers() {
        let mut mgr = CommitWindowManager::new(default_config());
        mgr.open(1.0);
        mgr.rollback_with_check(0).unwrap();
        assert!(matches!(mgr.state(), WindowState::Idle));
    }

    #[test]
    fn idle_has_zero_remaining() {
        let mgr = CommitWindowManager::new(default_config());
        assert_eq!(mgr.seconds_remaining(), 0);
    }
}
