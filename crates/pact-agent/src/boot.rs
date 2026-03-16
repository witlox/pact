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
use tokio_stream::StreamExt;
use tracing::{debug, info, warn};

use pact_common::config::AgentConfig;
use pact_common::proto::stream::{config_chunk, BootConfigRequest};
use pact_common::types::{
    ConfigState, DeltaItem, DriftWeights, StateDelta, SupervisorBackend, VClusterPolicy,
};

use crate::capability::{CapabilityReporter, GpuBackend, MockGpuBackend};
use crate::commit::CommitWindowManager;
use crate::conflict::ConflictManager;
use crate::drift::DriftEvaluator;
use crate::emergency::EmergencyManager;
use crate::journal_client::JournalClient;
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
    /// Cached vCluster policy from journal (None if not yet received).
    pub cached_policy: Arc<RwLock<Option<VClusterPolicy>>>,
}

/// Execute the agent boot sequence.
///
/// This is the main orchestration function called from `main()`.
/// Each phase logs its progress and timing for observability.
/// If `journal_client` is `Some`, the agent streams initial config from the journal.
/// If `None` (dev mode or disconnected start), it starts with defaults.
#[allow(clippy::too_many_lines)]
pub async fn boot(
    config: &AgentConfig,
    journal_client: Option<&JournalClient>,
) -> anyhow::Result<BootResult> {
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

    // Phase 7: Stream initial config from journal (if connected)
    let cached_policy = Arc::new(RwLock::new(None));
    if let Some(client) = journal_client {
        info!("Boot phase 7: streaming boot config from journal");
        match stream_boot_config(client, &config.node_id, &config.vcluster).await {
            Ok(boot_config) => {
                debug!(
                    overlay_bytes = boot_config.overlay_data.len(),
                    node_delta_bytes = boot_config.node_delta_data.len(),
                    base_version = boot_config.base_version,
                    "Boot config received from journal"
                );
                // Apply overlay data as state delta (Linux: sysctl + mounts)
                if !boot_config.overlay_data.is_empty() {
                    if let Ok(delta) =
                        serde_json::from_slice::<StateDelta>(&boot_config.overlay_data)
                    {
                        apply_state_delta(&delta);
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "Failed to stream boot config — starting with defaults");
            }
        }
    } else {
        info!("Boot phase 7: no journal connection — starting with defaults");
    }

    // Phase 8: Determine initial config state
    // ConfigState tracks the convergence lifecycle, not enforcement mode.
    // All agents start in ObserveOnly during bootstrap (ADR-002).
    let config_state = ConfigState::ObserveOnly;

    // Spawn auto-rollback timer (A4)
    let cw_timer = commit_window.clone();
    let em_timer = emergency.clone();
    let enforcement = config.enforcement_mode.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(10));
        loop {
            interval.tick().await;
            // D5: observe-only mode skips commit window enforcement
            if enforcement == "observe" {
                continue;
            }
            // Emergency mode suspends auto-rollback (A4 exception)
            if em_timer.read().await.is_active() {
                continue;
            }
            let mut cw = cw_timer.write().await;
            cw.check_expired();
            if matches!(cw.state(), crate::commit::WindowState::Expired { .. }) {
                tracing::warn!("Commit window expired — auto-rollback triggered (A4)");
                if let Err(e) = cw.rollback_with_check(0) {
                    tracing::warn!(error = %e, "Auto-rollback blocked");
                }
            }
        }
    });

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
        cached_policy,
    })
}

/// Result of streaming boot config from journal.
pub struct BootConfigData {
    /// Reassembled vCluster overlay data.
    pub overlay_data: Vec<u8>,
    /// Node-specific delta data.
    pub node_delta_data: Vec<u8>,
    /// Base overlay version.
    pub base_version: u64,
}

/// Stream boot config from journal's BootConfigService.
///
/// Receives overlay chunks + node delta + completion marker.
/// Reassembles chunked overlay data.
async fn stream_boot_config(
    client: &JournalClient,
    node_id: &str,
    vcluster_id: &str,
) -> anyhow::Result<BootConfigData> {
    let mut boot_client = client.boot_config();

    let response = boot_client
        .stream_boot_config(tonic::Request::new(BootConfigRequest {
            node_id: node_id.to_string(),
            vcluster_id: vcluster_id.to_string(),
            last_known_version: None, // First boot — no cached version
        }))
        .await
        .map_err(|e| anyhow::anyhow!("StreamBootConfig failed: {e}"))?;

    let mut stream = response.into_inner();
    let mut overlay_data = Vec::new();
    let mut node_delta_data = Vec::new();
    let mut base_version = 0u64;

    while let Some(result) = stream.next().await {
        let chunk = result.map_err(|e| anyhow::anyhow!("boot config stream error: {e}"))?;
        match chunk.chunk {
            Some(config_chunk::Chunk::BaseOverlay(ov)) => {
                debug!(
                    chunk_index = ov.chunk_index,
                    total_chunks = ov.total_chunks,
                    bytes = ov.data.len(),
                    "Received overlay chunk"
                );
                overlay_data.extend_from_slice(&ov.data);
                base_version = ov.version;
            }
            Some(config_chunk::Chunk::NodeDelta(nd)) => {
                debug!(node_id = %nd.node_id, bytes = nd.data.len(), "Received node delta");
                node_delta_data = nd.data;
            }
            Some(config_chunk::Chunk::Complete(c)) => {
                info!(
                    base_version = c.base_version,
                    node_version = c.node_version,
                    "Boot config stream complete"
                );
                break;
            }
            None => {}
        }
    }

    Ok(BootConfigData { overlay_data, node_delta_data, base_version })
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

/// Handles for subsystems that receive config updates.
pub struct ConfigActionHandles {
    pub drift_evaluator: Arc<RwLock<DriftEvaluator>>,
    pub commit_window: Arc<RwLock<CommitWindowManager>>,
    pub cached_policy: Arc<RwLock<Option<VClusterPolicy>>>,
    pub blacklist: pact_common::config::BlacklistConfig,
}

/// Process a config update action — dispatches to the appropriate subsystem.
pub async fn handle_config_action(action: ConfigUpdateAction, handles: &ConfigActionHandles) {
    match action {
        ConfigUpdateAction::OverlayChanged { data } => {
            info!(bytes = data.len(), "Received overlay update");
            // Overlay data is the full vCluster base config (compressed).
            // In a full implementation this would diff against current state
            // and apply changes (kernel params, mounts, modules, uenv).
            // For now, log receipt — actual application requires OS-level
            // primitives (nix crate) that are Linux-only.
        }
        ConfigUpdateAction::NodeDeltaChanged { data } => {
            info!(bytes = data.len(), "Received node delta update");
            // Node delta contains node-specific config entries.
            // Same as overlay — actual application requires OS primitives.
        }
        ConfigUpdateAction::PolicyChanged { policy } => {
            info!(vcluster = %policy.vcluster_id, "Policy updated from journal");

            // Update commit window config from policy
            {
                let mut cw = handles.commit_window.write().await;
                cw.update_config(
                    policy.base_commit_window_seconds,
                    policy.drift_sensitivity,
                    policy.emergency_window_seconds,
                );
            }

            // Update drift evaluator weights if policy sensitivity changed
            // (drift weights could be per-vCluster in future)

            // Cache the policy
            *handles.cached_policy.write().await = Some(policy);
        }
        ConfigUpdateAction::BlacklistChanged { patterns } => {
            info!(count = patterns.len(), "Blacklist updated from journal");
            let mut new_blacklist = handles.blacklist.clone();
            new_blacklist.patterns = patterns;
            let mut evaluator = handles.drift_evaluator.write().await;
            *evaluator = DriftEvaluator::new(new_blacklist, DriftWeights::default());
        }
    }
}

// ---------------------------------------------------------------------------
// Linux-specific config application
// ---------------------------------------------------------------------------

/// Apply sysctl entries by writing to /proc/sys.
///
/// Each `DeltaItem.key` is a sysctl key (e.g. `vm.swappiness`), and
/// `DeltaItem.value` is the value to write. Dots in the key are replaced
/// with slashes to form the /proc/sys path.
#[cfg(target_os = "linux")]
pub fn apply_sysctl(entries: &[DeltaItem]) {
    use pact_common::types::DeltaAction;

    for entry in entries {
        if entry.action == DeltaAction::Remove {
            warn!(key = %entry.key, "Cannot remove sysctl — skipping");
            continue;
        }
        let Some(ref value) = entry.value else {
            warn!(key = %entry.key, "Sysctl entry has no value — skipping");
            continue;
        };
        let path = format!("/proc/sys/{}", entry.key.replace('.', "/"));
        match std::fs::write(&path, value) {
            Ok(()) => info!(key = %entry.key, value = %value, "Applied sysctl"),
            Err(e) => {
                warn!(key = %entry.key, path = %path, error = %e, "Failed to apply sysctl — skipping");
            }
        }
    }
}

/// Apply mount entries.
///
/// For `DeltaAction::Add` / `DeltaAction::Modify`: mount using `nix::mount::mount()`.
/// The key is the mount path, value format is `"fstype:source (options)"`.
///
/// For `DeltaAction::Remove`: unmount the path via `nix::mount::umount()`.
#[cfg(target_os = "linux")]
pub fn apply_mounts(entries: &[DeltaItem]) {
    use nix::mount::{mount, umount, MsFlags};
    use pact_common::types::DeltaAction;

    for entry in entries {
        let mount_path = &entry.key;
        match entry.action {
            DeltaAction::Remove => match umount(mount_path.as_str()) {
                Ok(()) => info!(path = %mount_path, "Unmounted"),
                Err(e) => warn!(path = %mount_path, error = %e, "Failed to unmount — skipping"),
            },
            DeltaAction::Add | DeltaAction::Modify => {
                let Some(ref value) = entry.value else {
                    warn!(path = %mount_path, "Mount entry has no value — skipping");
                    continue;
                };
                // Parse value format: "fstype:source (options)"
                let (fs_type, source, options) = parse_mount_value(value);
                let flags = MsFlags::empty();
                match mount(
                    Some(source.as_str()),
                    mount_path.as_str(),
                    Some(fs_type.as_str()),
                    flags,
                    Some(options.as_str()),
                ) {
                    Ok(()) => info!(
                        path = %mount_path,
                        fs_type = %fs_type,
                        source = %source,
                        "Mounted"
                    ),
                    Err(e) => warn!(
                        path = %mount_path,
                        fs_type = %fs_type,
                        source = %source,
                        error = %e,
                        "Failed to mount — skipping"
                    ),
                }
            }
        }
    }
}

/// Parse mount value format: `"fstype:source (options)"`.
///
/// Returns `(fs_type, source, options)`. Missing parts default to empty strings.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_mount_value(value: &str) -> (String, String, String) {
    // Extract options from parentheses if present
    let (main, options) = if let Some(paren_start) = value.find('(') {
        let opts = value[paren_start + 1..].trim_end_matches(')').trim().to_string();
        (value[..paren_start].trim(), opts)
    } else {
        (value.trim(), String::new())
    };

    // Split "fstype:source"
    if let Some(colon) = main.find(':') {
        let fs_type = main[..colon].trim().to_string();
        let source = main[colon + 1..].trim().to_string();
        (fs_type, source, options)
    } else {
        (main.to_string(), String::new(), options)
    }
}

/// Apply a full state delta — orchestrates sysctl, mounts, and logs other categories.
#[cfg(target_os = "linux")]
pub fn apply_state_delta(delta: &StateDelta) {
    info!("Applying state delta");

    if !delta.kernel.is_empty() {
        info!(count = delta.kernel.len(), "Applying sysctl entries");
        apply_sysctl(&delta.kernel);
    }

    if !delta.mounts.is_empty() {
        info!(count = delta.mounts.len(), "Applying mount entries");
        apply_mounts(&delta.mounts);
    }

    // Categories that require additional tooling — log but do not apply
    if !delta.services.is_empty() {
        info!(
            count = delta.services.len(),
            "Service delta received — requires supervisor (not applied here)"
        );
    }
    if !delta.files.is_empty() {
        info!(
            count = delta.files.len(),
            "File delta received — requires file manager (not applied here)"
        );
    }
    if !delta.network.is_empty() {
        info!(
            count = delta.network.len(),
            "Network delta received — requires netlink manager (not applied here)"
        );
    }
    if !delta.packages.is_empty() {
        info!(
            count = delta.packages.len(),
            "Package delta received — requires package manager (not applied here)"
        );
    }
    if !delta.gpu.is_empty() {
        info!(
            count = delta.gpu.len(),
            "GPU delta received — requires GPU manager (not applied here)"
        );
    }

    let total = delta.kernel.len()
        + delta.mounts.len()
        + delta.services.len()
        + delta.files.len()
        + delta.network.len()
        + delta.packages.len()
        + delta.gpu.len();
    info!(
        total_items = total,
        applied_sysctl = delta.kernel.len(),
        applied_mounts = delta.mounts.len(),
        "State delta application complete"
    );
}

// --- Non-Linux stubs ---

/// Stub: sysctl application is Linux-only.
#[cfg(not(target_os = "linux"))]
pub fn apply_sysctl(entries: &[DeltaItem]) {
    if !entries.is_empty() {
        warn!(count = entries.len(), "apply_sysctl called on non-Linux platform — no-op");
    }
}

/// Stub: mount application is Linux-only.
#[cfg(not(target_os = "linux"))]
pub fn apply_mounts(entries: &[DeltaItem]) {
    if !entries.is_empty() {
        warn!(count = entries.len(), "apply_mounts called on non-Linux platform — no-op");
    }
}

/// Stub: state delta application is Linux-only.
#[cfg(not(target_os = "linux"))]
pub fn apply_state_delta(delta: &StateDelta) {
    let total = delta.kernel.len()
        + delta.mounts.len()
        + delta.services.len()
        + delta.files.len()
        + delta.network.len()
        + delta.packages.len()
        + delta.gpu.len();
    if total > 0 {
        warn!(total_items = total, "apply_state_delta called on non-Linux platform — no-op");
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
        let result = boot(&config, None).await.unwrap();

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
        let result = boot(&config, None).await.unwrap();

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
        let result = boot(&config, None).await.unwrap();

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
        let result = boot(&config, None).await.unwrap();

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

        let handles = ConfigActionHandles {
            drift_evaluator: evaluator.clone(),
            commit_window: Arc::new(RwLock::new(CommitWindowManager::new(
                CommitWindowConfig::default(),
            ))),
            cached_policy: Arc::new(RwLock::new(None)),
            blacklist: BlacklistConfig::default(),
        };

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
        handle_config_action(action, &handles).await;

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

    // --- parse_mount_value tests ---

    #[test]
    fn parse_mount_value_full_format() {
        let (fs, src, opts) = parse_mount_value("tmpfs:/dev/shm (size=64G,mode=1777)");
        assert_eq!(fs, "tmpfs");
        assert_eq!(src, "/dev/shm");
        assert_eq!(opts, "size=64G,mode=1777");
    }

    #[test]
    fn parse_mount_value_no_options() {
        let (fs, src, opts) = parse_mount_value("nfs:10.0.0.1:/export/data");
        assert_eq!(fs, "nfs");
        assert_eq!(src, "10.0.0.1:/export/data");
        assert_eq!(opts, "");
    }

    #[test]
    fn parse_mount_value_no_source() {
        let (fs, src, opts) = parse_mount_value("proc");
        assert_eq!(fs, "proc");
        assert_eq!(src, "");
        assert_eq!(opts, "");
    }

    #[test]
    fn apply_state_delta_empty_is_noop() {
        let delta = StateDelta {
            mounts: vec![],
            files: vec![],
            network: vec![],
            services: vec![],
            kernel: vec![],
            packages: vec![],
            gpu: vec![],
        };
        // Should not panic
        apply_state_delta(&delta);
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
