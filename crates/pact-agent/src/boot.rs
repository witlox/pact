//! Boot sequence orchestration — wires all agent subsystems together.
//!
//! Boot phases (target: <2s from agent start to node ready):
//! 1. Authenticate to journal (mTLS)
//! 2. Stream vCluster overlay + node delta
//! 3. Apply config: kernel params, modules, mounts, uenv
//! 4. Start declared services in dependency order
//! 5. Write CapabilityReport to tmpfs manifest
//! 6. Start config subscription for live updates
//! 7. Enter steady state: observer active, shell server listening

use std::path::Path;
use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use pact_common::config::AgentConfig;
use pact_common::types::{ConfigState, DriftWeights, SupervisorBackend};

use crate::capability::{CapabilityReporter, GpuBackend, MockGpuBackend};
use crate::commit::CommitWindowManager;
use crate::conflict::ConflictManager;
use crate::drift::DriftEvaluator;
use crate::emergency::EmergencyManager;
use crate::subscription::{ConfigSubscription, ConfigUpdateAction, SubscriptionConfig};
use crate::supervisor::{PactSupervisor, ServiceManager};

/// Boot result — subsystems initialized and ready.
pub struct BootResult {
    pub supervisor: Arc<dyn ServiceManager>,
    pub drift_evaluator: Arc<RwLock<DriftEvaluator>>,
    pub commit_window: Arc<RwLock<CommitWindowManager>>,
    pub emergency: Arc<RwLock<EmergencyManager>>,
    pub conflict_manager: Arc<RwLock<ConflictManager>>,
    pub config_state: ConfigState,
    pub enforcement_mode: String,
}

/// Execute the agent boot sequence.
///
/// This is the main orchestration function called from `main()`.
/// Each phase logs its progress and timing for observability.
pub async fn boot(config: &AgentConfig) -> anyhow::Result<BootResult> {
    let start = std::time::Instant::now();

    // Phase 1: Initialize supervisor
    info!("Boot phase 1: initializing process supervisor");
    let supervisor: Arc<dyn ServiceManager> = match config.supervisor.backend {
        SupervisorBackend::Pact => Arc::new(PactSupervisor::new()),
        SupervisorBackend::Systemd => {
            warn!("systemd backend requested but not compiled in — using pact supervisor");
            Arc::new(PactSupervisor::new())
        }
    };
    debug!(elapsed_ms = start.elapsed().as_millis(), "Supervisor initialized");

    // Phase 2: Initialize drift evaluator with blacklist from config
    info!("Boot phase 2: initializing drift evaluator");
    let drift_evaluator = Arc::new(RwLock::new(DriftEvaluator::new(
        config.blacklist.clone(),
        DriftWeights::default(),
    )));

    // Phase 3: Initialize commit window manager
    info!("Boot phase 3: initializing commit window manager");
    let commit_window =
        Arc::new(RwLock::new(CommitWindowManager::new(config.commit_window.clone())));

    // Phase 4: Initialize emergency manager
    let emergency =
        Arc::new(RwLock::new(EmergencyManager::new(config.commit_window.emergency_window_seconds)));

    // Phase 5: Initialize conflict manager (grace period = commit window duration)
    let conflict_manager =
        Arc::new(RwLock::new(ConflictManager::new(config.commit_window.base_window_seconds)));

    // Phase 6: Generate initial capability report
    info!("Boot phase 6: generating capability report");
    let gpu_backend: Box<dyn GpuBackend> = Box::new(MockGpuBackend::new());
    let reporter = CapabilityReporter::new(config.node_id.clone(), gpu_backend);
    let report = reporter.report().await?;

    // Write capability manifest (only if path is writable — skip on macOS dev)
    let manifest_path = Path::new("/run/pact/capability.json");
    if let Err(e) = reporter.write_manifest(&report, manifest_path).await {
        warn!(
            "Could not write capability manifest to {}: {e} (expected on macOS dev)",
            manifest_path.display()
        );
    }
    debug!(elapsed_ms = start.elapsed().as_millis(), "Capability report generated");

    // Phase 7: Determine initial config state
    // ConfigState tracks the convergence lifecycle, not enforcement mode.
    // All agents start in ObserveOnly during bootstrap (ADR-002).
    let config_state = ConfigState::ObserveOnly;

    let elapsed = start.elapsed();
    info!(
        elapsed_ms = elapsed.as_millis(),
        node_id = %config.node_id,
        vcluster = %config.vcluster,
        enforcement_mode = %config.enforcement_mode,
        config_state = ?config_state,
        "Boot sequence complete"
    );

    Ok(BootResult {
        supervisor,
        drift_evaluator,
        commit_window,
        emergency,
        conflict_manager,
        config_state,
        enforcement_mode: config.enforcement_mode.clone(),
    })
}

/// Start the config subscription for live updates from journal.
///
/// Returns the subscription handle and a receiver for config update actions.
/// Call this after boot to receive live config changes.
pub fn start_subscription(
    config: &AgentConfig,
) -> (Arc<ConfigSubscription>, tokio::sync::mpsc::Receiver<ConfigUpdateAction>) {
    let (action_tx, action_rx) = tokio::sync::mpsc::channel(64);

    let sub_config = SubscriptionConfig {
        node_id: config.node_id.clone(),
        vcluster_id: config.vcluster.clone(),
        ..Default::default()
    };

    let subscription = Arc::new(ConfigSubscription::new(sub_config, action_tx));
    (subscription, action_rx)
}

/// Process a config update action — dispatches to the appropriate subsystem.
pub async fn handle_config_action(
    action: ConfigUpdateAction,
    drift_evaluator: &Arc<RwLock<DriftEvaluator>>,
    blacklist: &pact_common::config::BlacklistConfig,
) {
    match action {
        ConfigUpdateAction::OverlayChanged { data } => {
            info!(bytes = data.len(), "Received overlay update — re-apply needed");
            // TODO: re-apply overlay diff (needs journal gRPC client)
        }
        ConfigUpdateAction::NodeDeltaChanged { data } => {
            info!(bytes = data.len(), "Received node delta update — re-apply needed");
            // TODO: re-apply node delta diff
        }
        ConfigUpdateAction::PolicyChanged { policy } => {
            info!(
                vcluster = %policy.vcluster_id,
                "Policy updated from journal"
            );
            // TODO: update cached policy, refresh commit window config
        }
        ConfigUpdateAction::BlacklistChanged { patterns } => {
            info!(count = patterns.len(), "Blacklist updated from journal");
            let mut new_blacklist = blacklist.clone();
            new_blacklist.patterns = patterns;
            let mut evaluator = drift_evaluator.write().await;
            *evaluator = DriftEvaluator::new(new_blacklist, DriftWeights::default());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commit::WindowState;
    use crate::observer::ObserverEvent;
    use pact_common::config::{
        AgentConfig, BlacklistConfig, CommitWindowConfig, JournalConnectionConfig, ObserverConfig,
        ShellConfig, SupervisorConfig,
    };
    use pact_common::types::SupervisorBackend;

    fn test_config() -> AgentConfig {
        AgentConfig {
            node_id: "test-node-001".into(),
            vcluster: "ml-training".into(),
            enforcement_mode: "observe".into(),
            supervisor: SupervisorConfig { backend: SupervisorBackend::Pact },
            journal: JournalConnectionConfig {
                endpoints: vec!["http://localhost:9443".into()],
                tls_enabled: false,
                tls_cert: None,
                tls_key: None,
                tls_ca: None,
            },
            observer: ObserverConfig::default(),
            shell: ShellConfig::default(),
            commit_window: CommitWindowConfig::default(),
            blacklist: BlacklistConfig::default(),
            capability: None,
        }
    }

    #[tokio::test]
    async fn boot_initializes_all_subsystems() {
        let config = test_config();
        let result = boot(&config).await.unwrap();

        // Config state is always ObserveOnly at boot (ADR-002)
        assert_eq!(result.config_state, ConfigState::ObserveOnly);
        assert_eq!(result.enforcement_mode, "observe");

        // Emergency should be inactive
        assert!(!result.emergency.read().await.is_active());

        // Conflict manager should have nothing pending
        assert!(result.conflict_manager.read().await.all_resolved());
        assert_eq!(result.conflict_manager.read().await.pending_count(), 0);

        // Commit window should be idle
        assert!(matches!(result.commit_window.read().await.state(), WindowState::Idle));

        // Drift evaluator should have zero magnitude
        assert_eq!(result.drift_evaluator.read().await.magnitude(), 0.0);
    }

    #[tokio::test]
    async fn boot_with_custom_commit_window_config() {
        let mut config = test_config();
        config.commit_window = CommitWindowConfig {
            base_window_seconds: 1800,
            drift_sensitivity: 3.0,
            emergency_window_seconds: 28800,
        };
        let result = boot(&config).await.unwrap();

        // Verify commit window uses the custom config
        let cw = result.commit_window.read().await;
        // With no drift, window should equal base_window: 1800 / (1 + 0*3) = 1800
        assert_eq!(cw.calculate_window_seconds(0.0), 1800);
        // With drift 1.0: 1800 / (1 + 1.0*3.0) = 1800/4 = 450
        assert_eq!(cw.calculate_window_seconds(1.0), 450);
    }

    #[tokio::test]
    async fn boot_with_custom_blacklist_wires_to_drift_evaluator() {
        let mut config = test_config();
        config.blacklist =
            BlacklistConfig { patterns: vec!["/scratch/**".into(), "/home/**".into()] };
        let result = boot(&config).await.unwrap();

        // Verify the evaluator uses the custom blacklist:
        // /scratch/job/output should be blacklisted (zero drift)
        let mut eval = result.drift_evaluator.write().await;
        let ev = ObserverEvent {
            category: "file".into(),
            path: "/scratch/job/output".into(),
            detail: "created".into(),
            timestamp: chrono::Utc::now(),
        };
        eval.process_event(&ev);
        assert_eq!(eval.drift_vector().files, 0.0, "/scratch should be blacklisted");

        // /etc/config should NOT be blacklisted
        let ev2 = ObserverEvent {
            category: "file".into(),
            path: "/etc/config".into(),
            detail: "modified".into(),
            timestamp: chrono::Utc::now(),
        };
        eval.process_event(&ev2);
        assert_eq!(eval.drift_vector().files, 1.0, "/etc should not be blacklisted");
    }

    #[tokio::test]
    async fn boot_supervisor_can_manage_processes() {
        let config = test_config();
        let result = boot(&config).await.unwrap();

        // Verify the supervisor is functional — start and stop a real process
        let svc = pact_common::types::ServiceDecl {
            name: "boot-test-sleep".into(),
            binary: "sleep".into(),
            args: vec!["60".into()],
            restart: pact_common::types::RestartPolicy::Never,
            restart_delay_seconds: 0,
            depends_on: vec![],
            order: 0,
            cgroup_memory_max: None,
            health_check: None,
        };

        result.supervisor.start(&svc).await.unwrap();
        let status = result.supervisor.status(&svc).await.unwrap();
        assert_eq!(status.state, pact_common::types::ServiceState::Running);
        assert!(status.pid.is_some());

        result.supervisor.stop(&svc).await.unwrap();
        let status = result.supervisor.status(&svc).await.unwrap();
        assert_eq!(status.state, pact_common::types::ServiceState::Stopped);
    }

    #[tokio::test]
    async fn handle_blacklist_action_changes_drift_filtering() {
        // Start with default blacklist (includes /tmp/**)
        let evaluator = Arc::new(RwLock::new(DriftEvaluator::new(
            BlacklistConfig::default(),
            DriftWeights::default(),
        )));
        let blacklist = BlacklistConfig::default();

        // Verify /tmp is blacklisted initially
        {
            let mut eval = evaluator.write().await;
            eval.process_event(&ObserverEvent {
                category: "file".into(),
                path: "/tmp/test".into(),
                detail: "created".into(),
                timestamp: chrono::Utc::now(),
            });
            assert_eq!(eval.drift_vector().files, 0.0, "/tmp should be blacklisted initially");
        }

        // Apply a blacklist change that does NOT include /tmp
        let action = ConfigUpdateAction::BlacklistChanged { patterns: vec!["/scratch/**".into()] };
        handle_config_action(action, &evaluator, &blacklist).await;

        // Now /tmp should NOT be blacklisted anymore
        {
            let mut eval = evaluator.write().await;
            eval.process_event(&ObserverEvent {
                category: "file".into(),
                path: "/tmp/test".into(),
                detail: "created".into(),
                timestamp: chrono::Utc::now(),
            });
            assert_eq!(
                eval.drift_vector().files,
                1.0,
                "/tmp should NOT be blacklisted after blacklist update"
            );
        }
    }

    #[tokio::test]
    async fn start_subscription_wires_config() {
        let config = test_config();
        let (sub, _rx) = start_subscription(&config);
        assert_eq!(sub.config().node_id, "test-node-001");
        assert_eq!(sub.config().vcluster_id, "ml-training");
        // Verify initial state
        let state = sub.state().await;
        assert_eq!(state.last_sequence, 0);
        assert!(!state.connected);
        assert_eq!(state.reconnect_attempts, 0);
    }
}
