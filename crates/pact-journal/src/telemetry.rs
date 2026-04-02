//! Telemetry server — Prometheus metrics + health endpoint.
//!
//! Runs on port 9091 (not 9090 to avoid Prometheus server default conflict).
//! Per ADR-005: only 3-5 scrape targets, minimal overhead.

use std::sync::Arc;

use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use openraft::Raft;
use prometheus::{Encoder, IntGauge, TextEncoder};
use tokio::sync::RwLock;

use openraft::async_runtime::watch::WatchReceiver;

use crate::raft::types::JournalTypeConfig;
use crate::JournalState;

/// Shared state for the telemetry HTTP server.
#[derive(Clone)]
pub struct TelemetryState {
    pub raft: Raft<JournalTypeConfig>,
    pub journal: Arc<RwLock<JournalState>>,
    pub metrics: JournalMetrics,
    /// IdP URL for auth discovery (PAuth3). Empty if not configured.
    pub idp_url: String,
    /// Public client ID for auth discovery.
    pub client_id: String,
}

/// Prometheus metrics for the journal.
#[derive(Clone)]
pub struct JournalMetrics {
    // Config metrics
    pub entries_total: IntGauge,
    pub boot_streams_active: IntGauge,
    pub overlay_builds_total: IntGauge,
    // Raft metrics
    pub raft_leader: IntGauge,
    pub raft_term: IntGauge,
    pub raft_log_entries: IntGauge,
    pub raft_replication_lag: IntGauge,
}

impl JournalMetrics {
    pub fn new() -> Self {
        let entries_total = IntGauge::new(
            "pact_journal_entries_total",
            "Total number of config entries in the journal",
        )
        .unwrap();
        let boot_streams_active = IntGauge::new(
            "pact_journal_boot_streams_active",
            "Number of active boot config streams",
        )
        .unwrap();
        let overlay_builds_total =
            IntGauge::new("pact_journal_overlay_builds_total", "Total number of overlay builds")
                .unwrap();
        let raft_leader =
            IntGauge::new("pact_raft_leader", "Current Raft leader node ID (-1 if none)").unwrap();
        let raft_term = IntGauge::new("pact_raft_term", "Current Raft term").unwrap();
        let raft_log_entries =
            IntGauge::new("pact_raft_log_entries", "Number of committed Raft log entries").unwrap();
        let raft_replication_lag = IntGauge::new(
            "pact_raft_replication_lag",
            "Raft replication lag (entries behind leader)",
        )
        .unwrap();

        // Ignore AlreadyReg errors (metrics are global singletons)
        let _ = prometheus::register(Box::new(entries_total.clone()));
        let _ = prometheus::register(Box::new(boot_streams_active.clone()));
        let _ = prometheus::register(Box::new(overlay_builds_total.clone()));
        let _ = prometheus::register(Box::new(raft_leader.clone()));
        let _ = prometheus::register(Box::new(raft_term.clone()));
        let _ = prometheus::register(Box::new(raft_log_entries.clone()));
        let _ = prometheus::register(Box::new(raft_replication_lag.clone()));

        Self {
            entries_total,
            boot_streams_active,
            overlay_builds_total,
            raft_leader,
            raft_term,
            raft_log_entries,
            raft_replication_lag,
        }
    }
}

impl Default for JournalMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Build the axum router for telemetry endpoints.
///
/// Includes `/auth/discovery` (PAuth3: public, no auth required).
pub fn telemetry_router(state: TelemetryState) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/metrics", get(metrics_handler))
        .route("/auth/discovery", get(auth_discovery_handler))
        .with_state(state)
}

/// Health endpoint: returns Raft role and basic status.
async fn health_handler(State(state): State<TelemetryState>) -> impl IntoResponse {
    let metrics = state.raft.metrics();
    let server_state = metrics.borrow_watched().state;
    let role = match server_state {
        openraft::ServerState::Leader => "leader",
        openraft::ServerState::Follower => "follower",
        openraft::ServerState::Candidate => "candidate",
        openraft::ServerState::Learner => "learner",
        openraft::ServerState::Shutdown => "shutdown",
    };
    let journal = state.journal.read().await;
    let entries = journal.entries.len();
    drop(journal);

    axum::Json(serde_json::json!({
        "status": "healthy",
        "role": role,
        "entries": entries,
    }))
}

/// Prometheus metrics endpoint.
async fn metrics_handler(State(state): State<TelemetryState>) -> impl IntoResponse {
    // Update config gauges from journal state
    let journal = state.journal.read().await;
    state.metrics.entries_total.set(i64::try_from(journal.entries.len()).unwrap_or(i64::MAX));
    drop(journal);

    // Update Raft gauges from openraft metrics
    let raft_metrics = state.raft.metrics();
    let snapshot = raft_metrics.borrow_watched();
    if let Some(leader_id) = snapshot.current_leader {
        state.metrics.raft_leader.set(i64::try_from(leader_id).unwrap_or(-1));
    } else {
        state.metrics.raft_leader.set(-1);
    }
    state.metrics.raft_term.set(i64::try_from(snapshot.current_term).unwrap_or(0));
    if let Some(ref last_applied) = snapshot.last_applied {
        state.metrics.raft_log_entries.set(i64::try_from(last_applied.index).unwrap_or(0));
    }
    // Replication lag: on a single node, lag is 0. In a cluster, computed from
    // leader's last_log_index vs this node's last_applied.
    // openraft 0.10.0-alpha.14 doesn't expose commit index directly.
    state.metrics.raft_replication_lag.set(0);

    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    let mut buffer = Vec::new();
    encoder.encode(&metric_families, &mut buffer).unwrap();
    let content_type = encoder.format_type().to_string();
    ([(axum::http::header::CONTENT_TYPE, content_type)], buffer)
}

/// Auth discovery endpoint (PAuth3: public, no auth required).
///
/// Returns IdP URL and client ID for CLI login flow.
async fn auth_discovery_handler(State(state): State<TelemetryState>) -> impl IntoResponse {
    axum::Json(serde_json::json!({
        "idp_url": state.idp_url,
        "client_id": state.client_id,
        "scopes": ["openid", "profile"],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::StatusCode;
    use openraft::Raft;
    use pact_common::types::EntryType;
    use raft_hpc_core::{FileLogStore, GrpcNetworkFactory, HpcStateMachine, StateMachineState};
    use tower::ServiceExt;

    use crate::raft::types::JournalCommand;

    fn test_entry(entry_type: EntryType) -> pact_common::types::ConfigEntry {
        pact_common::types::ConfigEntry {
            sequence: 0,
            timestamp: chrono::Utc::now(),
            entry_type,
            scope: pact_common::types::Scope::Global,
            author: pact_common::types::Identity {
                principal: "admin@example.com".into(),
                principal_type: pact_common::types::PrincipalType::Human,
                role: "pact-platform-admin".into(),
            },
            parent: None,
            state_delta: None,
            policy_ref: None,
            ttl_seconds: None,
            emergency_reason: None,
        }
    }

    async fn test_telemetry_state() -> (TelemetryState, tempfile::TempDir) {
        let mut journal_state = JournalState::default();
        journal_state.apply(JournalCommand::AppendEntry(test_entry(EntryType::Commit)));
        journal_state.apply(JournalCommand::AppendEntry(test_entry(EntryType::Rollback)));

        let journal = Arc::new(RwLock::new(journal_state));
        let temp = tempfile::tempdir().unwrap();
        let config = Arc::new(
            openraft::Config {
                heartbeat_interval: 500,
                election_timeout_min: 1500,
                election_timeout_max: 3000,
                ..Default::default()
            }
            .validate()
            .unwrap(),
        );
        let log_store = FileLogStore::<JournalTypeConfig>::new(temp.path()).unwrap();
        let snapshot_dir = temp.path().join("snapshots");
        std::fs::create_dir_all(&snapshot_dir).unwrap();
        let sm = HpcStateMachine::with_snapshot_dir(Arc::clone(&journal), snapshot_dir).unwrap();
        let network = GrpcNetworkFactory::new();
        let raft = Raft::new(1, config, network, log_store, sm).await.unwrap();

        let state = TelemetryState {
            raft,
            journal,
            metrics: JournalMetrics::default(),
            idp_url: "https://test-idp.example.com".into(),
            client_id: "pact-cli-test".into(),
        };
        (state, temp)
    }

    #[tokio::test]
    async fn health_returns_200_with_role() {
        let (state, _tmp) = test_telemetry_state().await;
        let app = telemetry_router(state);

        let resp = app
            .oneshot(axum::http::Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "healthy");
        assert!(json["role"].is_string());
        assert_eq!(json["entries"], 2);
    }

    #[tokio::test]
    async fn metrics_returns_prometheus_format() {
        let (state, _tmp) = test_telemetry_state().await;
        let app = telemetry_router(state);

        let resp = app
            .oneshot(axum::http::Request::builder().uri("/metrics").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();
        assert!(text.contains("pact_journal_entries_total"));
    }
}
