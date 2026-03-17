//! Heartbeat monitor — detects inactive nodes by subscription stream liveness.
//!
//! Scans Active enrollments and writes `DeactivateNode` commands when a node
//! hasn't been seen within the configured heartbeat timeout.

use std::sync::Arc;

use openraft::Raft;
use tokio::sync::RwLock;
use tracing::{info, warn};

use pact_common::types::EnrollmentState;

use crate::raft::types::{JournalCommand, JournalTypeConfig};
use crate::JournalState;

/// Monitors node liveness and deactivates timed-out nodes.
pub struct HeartbeatMonitor {
    raft: Raft<JournalTypeConfig>,
    state: Arc<RwLock<JournalState>>,
    timeout_seconds: u32,
}

impl HeartbeatMonitor {
    pub fn new(
        raft: Raft<JournalTypeConfig>,
        state: Arc<RwLock<JournalState>>,
        timeout_seconds: u32,
    ) -> Self {
        Self { raft, state, timeout_seconds }
    }

    /// Start the heartbeat monitoring loop. Runs until cancelled.
    pub async fn run(&self) {
        let check_interval = tokio::time::Duration::from_secs(u64::from(self.timeout_seconds) / 2);
        let mut interval = tokio::time::interval(check_interval);

        loop {
            interval.tick().await;
            self.check_timeouts().await;
        }
    }

    /// Scan all Active enrollments and deactivate timed-out nodes.
    async fn check_timeouts(&self) {
        let state = self.state.read().await;
        let now = chrono::Utc::now();
        let timeout = chrono::Duration::seconds(i64::from(self.timeout_seconds));

        let timed_out: Vec<String> = state
            .enrollments
            .values()
            .filter(|e| e.state == EnrollmentState::Active)
            .filter(|e| e.last_seen.is_some_and(|ls| now.signed_duration_since(ls) > timeout))
            .map(|e| e.node_id.clone())
            .collect();
        drop(state);

        for node_id in timed_out {
            warn!(node_id = %node_id, "Node heartbeat timeout — deactivating");
            let cmd = JournalCommand::DeactivateNode { node_id: node_id.clone() };
            match self.raft.client_write(cmd).await {
                Ok(_) => info!(node_id = %node_id, "Node deactivated via heartbeat timeout"),
                Err(e) => warn!(node_id = %node_id, error = %e, "Failed to deactivate node"),
            }
        }
    }
}
