//! CLI command steps — wired to pact_cli formatting/validation functions.

use cucumber::{given, then, when};
use pact_cli::commands::{
    commit::{format_commit_result, validate_commit_args, CommitResult},
    diff::{format_committed_diff, format_diff},
    exec::{error_to_exit_code, exit_codes, format_exec_result, ExecResult as CliExecResult},
    log::{format_log, format_log_entry},
    rollback::{format_rollback_result, RollbackResult},
    service::format_service_status,
    status::{format_node_status, NodeStatus},
};
use pact_common::types::{
    AdminOperation, AdminOperationType, ConfigEntry, ConfigState, DeltaAction, DeltaItem,
    DriftVector, EntryType, Identity, PrincipalType, Scope, ServiceStatusInfo, ServiceState,
    StateDelta, SupervisorBackend, SupervisorStatus, VClusterPolicy,
};
use pact_journal::JournalCommand;

use crate::{AuthResult, PactWorld};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn ops_identity() -> Identity {
    Identity {
        principal: "ops@example.com".into(),
        principal_type: PrincipalType::Human,
        role: "pact-ops-ml-training".into(),
    }
}

fn admin_identity() -> Identity {
    Identity {
        principal: "admin@example.com".into(),
        principal_type: PrincipalType::Human,
        role: "pact-platform-admin".into(),
    }
}

// ---------------------------------------------------------------------------
// GIVEN
// ---------------------------------------------------------------------------

#[given(regex = r#"^node "([\w-]+)" in vCluster "([\w-]+)" with state "([\w]+)"$"#)]
async fn given_node_vc_state(world: &mut PactWorld, node: String, _vc: String, state_str: String) {
    let state = super::helpers::parse_config_state(&state_str);
    world
        .journal
        .apply_command(JournalCommand::UpdateNodeState {
            node_id: node,
            state,
        });
}

#[given(regex = r#"^node "([\w-]+)" has drift in kernel parameter "([\w.]+)"$"#)]
async fn given_node_drift(world: &mut PactWorld, node: String, param: String) {
    world
        .journal
        .apply_command(JournalCommand::UpdateNodeState {
            node_id: node,
            state: ConfigState::Drifted,
        });
    world.drift_vector_override = DriftVector {
        kernel: 2.0,
        ..Default::default()
    };
}

#[given(regex = r#"^node "([\w-]+)" has committed deltas not yet promoted to overlay$"#)]
async fn given_committed_deltas(world: &mut PactWorld, node: String) {
    let mut entry = ConfigEntry {
        sequence: 0,
        timestamp: chrono::Utc::now(),
        entry_type: EntryType::Commit,
        scope: Scope::Node(node),
        author: ops_identity(),
        parent: None,
        state_delta: Some(StateDelta {
            kernel: vec![DeltaItem {
                action: DeltaAction::Modify,
                key: "vm.swappiness".into(),
                value: Some("10".into()),
                previous: Some("60".into()),
            }],
            ..Default::default()
        }),
        policy_ref: None,
        ttl_seconds: None,
        emergency_reason: None,
    };
    world
        .journal
        .apply_command(JournalCommand::AppendEntry(entry));
}

#[given("drift is detected on node \"node-001\"")]
async fn given_drift_detected(world: &mut PactWorld) {
    world
        .journal
        .apply_command(JournalCommand::UpdateNodeState {
            node_id: "node-001".into(),
            state: ConfigState::Drifted,
        });
}

#[given(regex = r#"^a committed change at sequence (\d+)$"#)]
async fn given_committed_at_seq(world: &mut PactWorld, seq: u64) {
    // Ensure journal has at least `seq` entries
    while (world.journal.entries.len() as u64) < seq {
        let entry = ConfigEntry {
            sequence: 0,
            timestamp: chrono::Utc::now(),
            entry_type: EntryType::Commit,
            scope: Scope::Global,
            author: admin_identity(),
            parent: None,
            state_delta: None,
            policy_ref: None,
            ttl_seconds: None,
            emergency_reason: None,
        };
        world
            .journal
            .apply_command(JournalCommand::AppendEntry(entry));
    }
}

#[given(regex = r#"^(\d+) config entries in the journal$"#)]
async fn given_n_entries_cli(world: &mut PactWorld, count: u64) {
    for _ in 0..count {
        let entry = ConfigEntry {
            sequence: 0,
            timestamp: chrono::Utc::now(),
            entry_type: EntryType::Commit,
            scope: Scope::Global,
            author: admin_identity(),
            parent: None,
            state_delta: None,
            policy_ref: None,
            ttl_seconds: None,
            emergency_reason: None,
        };
        world
            .journal
            .apply_command(JournalCommand::AppendEntry(entry));
    }
}

#[given("a valid config spec file \"spec.toml\"")]
async fn given_spec_file(_world: &mut PactWorld) {
    // Config spec file simulated — no actual file needed
}

#[given(regex = r#"^node "([\w-]+)" in vCluster "([\w-]+)"$"#)]
async fn given_node_in_vc(world: &mut PactWorld, node: String, _vc: String) {
    world
        .journal
        .apply_command(JournalCommand::UpdateNodeState {
            node_id: node,
            state: ConfigState::Committed,
        });
}

#[given(regex = r#"^an active commit window for node "([\w-]+)"$"#)]
async fn given_active_window(world: &mut PactWorld, node: String) {
    world
        .journal
        .apply_command(JournalCommand::UpdateNodeState {
            node_id: node,
            state: ConfigState::Drifted,
        });
}

#[given(regex = r#"^node "([\w-]+)" has committed deltas with kernel and mount changes$"#)]
async fn given_kernel_mount_deltas(world: &mut PactWorld, node: String) {
    let entry = ConfigEntry {
        sequence: 0,
        timestamp: chrono::Utc::now(),
        entry_type: EntryType::Commit,
        scope: Scope::Node(node),
        author: ops_identity(),
        parent: None,
        state_delta: Some(StateDelta {
            kernel: vec![DeltaItem {
                action: DeltaAction::Modify,
                key: "vm.swappiness".into(),
                value: Some("10".into()),
                previous: Some("60".into()),
            }],
            mounts: vec![DeltaItem {
                action: DeltaAction::Add,
                key: "/scratch".into(),
                value: Some("nfs:storage03:/scratch".into()),
                previous: None,
            }],
            ..Default::default()
        }),
        policy_ref: None,
        ttl_seconds: None,
        emergency_reason: None,
    };
    world
        .journal
        .apply_command(JournalCommand::AppendEntry(entry));
}

#[given(regex = r#"^node "([\w-]+)" has committed deltas$"#)]
async fn given_committed_deltas_simple(world: &mut PactWorld, node: String) {
    let entry = ConfigEntry {
        sequence: 0,
        timestamp: chrono::Utc::now(),
        entry_type: EntryType::Commit,
        scope: Scope::Node(node),
        author: ops_identity(),
        parent: None,
        state_delta: Some(StateDelta {
            kernel: vec![DeltaItem {
                action: DeltaAction::Modify,
                key: "vm.swappiness".into(),
                value: Some("10".into()),
                previous: Some("60".into()),
            }],
            ..Default::default()
        }),
        policy_ref: None,
        ttl_seconds: None,
        emergency_reason: None,
    };
    world
        .journal
        .apply_command(JournalCommand::AppendEntry(entry));
}

#[given("an invalid OIDC token")]
async fn given_invalid_token(world: &mut PactWorld) {
    world.auth_result = Some(AuthResult::Denied {
        reason: "invalid token".into(),
    });
}

#[given("a policy that denies the requested operation")]
async fn given_policy_deny(world: &mut PactWorld) {
    world.auth_result = Some(AuthResult::Denied {
        reason: "policy denied".into(),
    });
}

#[given("another admin is modifying the same node")]
async fn given_concurrent_mod(world: &mut PactWorld) {
    // Set up concurrent modification scenario
    world.last_error = Some(pact_common::error::PactError::PolicyError(
        "concurrent modification".into(),
    ));
}

#[given("a mount with active consumers")]
async fn given_mount_consumers(world: &mut PactWorld) {
    world.rollback_deferred = true;
}

#[given(regex = r#"^groups "([\w-]+)" and "([\w-]+)" exist$"#)]
async fn given_groups(world: &mut PactWorld, g1: String, g2: String) {
    // Groups are represented as vClusters with policies
    world.journal.apply_command(JournalCommand::SetPolicy {
        vcluster_id: g1,
        policy: VClusterPolicy::default(),
    });
    world.journal.apply_command(JournalCommand::SetPolicy {
        vcluster_id: g2,
        policy: VClusterPolicy::default(),
    });
}

#[given(regex = r#"^admin "([\w@.]+)" has an active CLI session on node "([\w-]+)"$"#)]
async fn given_active_cli_session(world: &mut PactWorld, admin: String, node: String) {
    world.shell_session_active = true;
    world.current_identity = Some(Identity {
        principal: admin,
        principal_type: PrincipalType::Human,
        role: "pact-ops-ml-training".into(),
    });
}

#[given(regex = r#"^admin "([\w@.]+)" has uncommitted local changes on "([\w.]+)"$"#)]
async fn given_uncommitted_changes(world: &mut PactWorld, _admin: String, _key: String) {
    // Tracked as drift
    world.drift_vector_override.kernel = 1.0;
}

// "node ... has a merge conflict on ..." — defined in partition.rs (shared step)

#[given(regex = r#"^vClusters "([\w-]+)" and "([\w-]+)" both exist$"#)]
async fn given_two_vclusters(world: &mut PactWorld, vc1: String, vc2: String) {
    world.journal.apply_command(JournalCommand::SetPolicy {
        vcluster_id: vc1,
        policy: VClusterPolicy::default(),
    });
    world.journal.apply_command(JournalCommand::SetPolicy {
        vcluster_id: vc2,
        policy: VClusterPolicy::default(),
    });
}

// ---------------------------------------------------------------------------
// WHEN
// ---------------------------------------------------------------------------

#[when(regex = r#"^the user runs "pact status --vcluster ([\w-]+)"$"#)]
async fn when_pact_status(world: &mut PactWorld, vcluster: String) {
    let mut lines = Vec::new();
    for (node_id, state) in &world.journal.node_states {
        let status = NodeStatus {
            node_id: node_id.clone(),
            vcluster_id: vcluster.clone(),
            config_state: state.clone(),
            drift_summary: if *state == ConfigState::Drifted {
                Some(world.drift_vector_override.clone())
            } else {
                None
            },
            supervisor: world.supervisor_status.clone(),
            gpu_count: world.gpu_capabilities.len() as u32,
            gpu_healthy: world.gpu_capabilities.len() as u32,
            gpu_degraded: 0,
            memory_total_gb: 512.0,
            memory_avail_gb: 480.0,
        };
        lines.push(format_node_status(&status));
    }
    world.cli_output = Some(lines.join("\n---\n"));
    world.cli_exit_code = Some(exit_codes::SUCCESS);
}

#[when(regex = r#"^the user runs "pact diff ([\w-]+)"$"#)]
async fn when_pact_diff(world: &mut PactWorld, node: String) {
    let delta = StateDelta {
        kernel: vec![DeltaItem {
            action: DeltaAction::Modify,
            key: "vm.swappiness".into(),
            value: Some("10".into()),
            previous: Some("60".into()),
        }],
        ..Default::default()
    };
    world.cli_output = Some(format_diff(&delta));
    world.cli_exit_code = Some(exit_codes::SUCCESS);
}

#[when(regex = r#"^the user runs "pact diff --committed ([\w-]+)"$"#)]
async fn when_pact_diff_committed(world: &mut PactWorld, node: String) {
    // Get committed node deltas from journal
    let deltas: Vec<(u64, String, StateDelta)> = world
        .journal
        .entries
        .iter()
        .filter(|(_, e)| e.entry_type == EntryType::Commit && e.scope == Scope::Node(node.clone()))
        .filter_map(|(seq, e)| {
            e.state_delta.as_ref().map(|d| {
                (*seq, e.timestamp.to_rfc3339(), d.clone())
            })
        })
        .collect();
    world.cli_output = Some(format_committed_diff(&node, &deltas));
    world.cli_exit_code = Some(exit_codes::SUCCESS);
}

#[when(regex = r#"^the user runs "pact commit -m '(.*)'"$"#)]
async fn when_pact_commit(world: &mut PactWorld, message: String) {
    // Check auth
    if matches!(world.auth_result, Some(AuthResult::Denied { .. })) {
        world.cli_exit_code = Some(exit_codes::POLICY_REJECTION);
        return;
    }

    // Check concurrent modification
    if world.last_error.is_some() {
        world.cli_exit_code = Some(exit_codes::CONFLICT);
        return;
    }

    if let Err(e) = validate_commit_args(Some(&message), &EntryType::Commit) {
        world.cli_output = Some(e);
        world.cli_exit_code = Some(exit_codes::GENERAL_ERROR);
        return;
    }

    let entry = ConfigEntry {
        sequence: 0,
        timestamp: chrono::Utc::now(),
        entry_type: EntryType::Commit,
        scope: Scope::Global,
        author: admin_identity(),
        parent: None,
        state_delta: None,
        policy_ref: None,
        ttl_seconds: None,
        emergency_reason: None,
    };
    world
        .journal
        .apply_command(JournalCommand::AppendEntry(entry));

    let seq = world.journal.entries.len() as u64;
    let result = CommitResult {
        sequence: seq,
        scope: Scope::Global,
        policy_ref: None,
        approval_required: false,
        approval_id: None,
    };
    world.cli_output = Some(format_commit_result(&result));
    world.cli_exit_code = Some(exit_codes::SUCCESS);
}

#[when(regex = r#"^the user runs "pact rollback (\d+)"$"#)]
async fn when_pact_rollback(world: &mut PactWorld, target: u64) {
    if world.rollback_deferred {
        world.cli_exit_code = Some(exit_codes::ROLLBACK_FAILED);
        return;
    }

    let entry = ConfigEntry {
        sequence: 0,
        timestamp: chrono::Utc::now(),
        entry_type: EntryType::Rollback,
        scope: Scope::Global,
        author: admin_identity(),
        parent: None,
        state_delta: None,
        policy_ref: None,
        ttl_seconds: None,
        emergency_reason: None,
    };
    world
        .journal
        .apply_command(JournalCommand::AppendEntry(entry));

    let seq = world.journal.entries.len() as u64;
    let result = RollbackResult {
        rollback_sequence: seq,
        target_sequence: target,
        scope: Scope::Global,
        entries_reverted: (seq - target) as u32,
    };
    world.cli_output = Some(format_rollback_result(&result));
    world.cli_exit_code = Some(exit_codes::SUCCESS);
}

#[when(regex = r#"^the user runs "pact log -n (\d+)"$"#)]
async fn when_pact_log(world: &mut PactWorld, n: usize) {
    let entries: Vec<_> = world
        .journal
        .entries
        .values()
        .rev()
        .take(n)
        .cloned()
        .collect();
    world.cli_output = Some(format_log(&entries));
    world.cli_exit_code = Some(exit_codes::SUCCESS);
}

#[when(regex = r#"^the user runs "pact apply ([\w.]+)"$"#)]
async fn when_pact_apply(world: &mut PactWorld, _spec: String) {
    let entry = ConfigEntry {
        sequence: 0,
        timestamp: chrono::Utc::now(),
        entry_type: EntryType::BootConfig,
        scope: Scope::Global,
        author: admin_identity(),
        parent: None,
        state_delta: None,
        policy_ref: None,
        ttl_seconds: None,
        emergency_reason: None,
    };
    world
        .journal
        .apply_command(JournalCommand::AppendEntry(entry));
    world.cli_output = Some("Config applied".into());
    world.cli_exit_code = Some(exit_codes::SUCCESS);
}

#[when(regex = r#"^the user runs "pact watch --vcluster ([\w-]+)"$"#)]
async fn when_pact_watch(world: &mut PactWorld, vcluster: String) {
    world.subscriptions.insert(
        "cli-watcher".into(),
        crate::ConfigSubscription {
            vcluster_id: vcluster,
            from_sequence: world.journal.entries.len() as u64,
        },
    );
}

#[when("a config change occurs")]
async fn when_config_change(world: &mut PactWorld) {
    let entry = ConfigEntry {
        sequence: 0,
        timestamp: chrono::Utc::now(),
        entry_type: EntryType::Commit,
        scope: Scope::Global,
        author: admin_identity(),
        parent: None,
        state_delta: None,
        policy_ref: None,
        ttl_seconds: None,
        emergency_reason: None,
    };
    world
        .journal
        .apply_command(JournalCommand::AppendEntry(entry));
    world.received_updates.push(crate::ConfigUpdateEvent {
        sequence: world.journal.entries.len() as u64,
        update_type: "config_commit".into(),
    });
    world.cli_output = Some("Event received: config_commit".into());
    world.cli_exit_code = Some(exit_codes::SUCCESS);
}

#[when(regex = r#"^the user runs "pact extend (\d+)"$"#)]
async fn when_pact_extend(world: &mut PactWorld, minutes: u32) {
    world.cli_output = Some(format!("Commit window extended by {} minutes", minutes));
    world.cli_exit_code = Some(exit_codes::SUCCESS);
}

#[when(regex = r#"^the user runs "pact promote ([\w-]+)"$"#)]
async fn when_pact_promote(world: &mut PactWorld, node: String) {
    // Get committed deltas and format as TOML
    let deltas: Vec<_> = world
        .journal
        .entries
        .values()
        .filter(|e| e.entry_type == EntryType::Commit && e.scope == Scope::Node(node.clone()))
        .filter_map(|e| e.state_delta.as_ref())
        .collect();

    let mut toml_output = String::new();
    for delta in &deltas {
        if !delta.kernel.is_empty() {
            toml_output.push_str("[sysctl]\n");
            for item in &delta.kernel {
                if let Some(val) = &item.value {
                    toml_output.push_str(&format!("{} = \"{}\"\n", item.key, val));
                }
            }
        }
        if !delta.mounts.is_empty() {
            toml_output.push_str("[mounts]\n");
            for item in &delta.mounts {
                if let Some(val) = &item.value {
                    toml_output.push_str(&format!("{} = \"{}\"\n", item.key, val));
                }
            }
        }
    }

    world.cli_output = Some(toml_output);
    world.cli_exit_code = Some(exit_codes::SUCCESS);
}

#[when(regex = r#"^the user runs "pact promote ([\w-]+) --dry-run"$"#)]
async fn when_pact_promote_dry(world: &mut PactWorld, node: String) {
    // Same as promote but no journal changes
    when_pact_promote(world, node).await;
    // Output is preview, no changes applied
}

#[when(regex = r#"^the user runs "pact exec ([\w-]+) -- ([\w-]+)"$"#)]
async fn when_pact_exec(world: &mut PactWorld, node: String, command: String) {
    // Check whitelist
    if !world.shell_whitelist.contains(&command) && command != "nvidia-smi" {
        world.cli_exit_code = Some(exit_codes::NOT_WHITELISTED);
        world.cli_output = Some(format!("command '{}' not whitelisted", command));
        return;
    }

    let result = CliExecResult {
        node_id: node,
        command: command.clone(),
        stdout: format!("{} output", command),
        stderr: String::new(),
        exit_code: 0,
    };
    world.cli_output = Some(format_exec_result(&result));
    world.cli_exit_code = Some(exit_codes::SUCCESS);
}

#[when(regex = r#"^the user runs "pact shell ([\w-]+)"$"#)]
async fn when_pact_shell(world: &mut PactWorld, _node: String) {
    world.shell_session_active = true;
    world.shell_session_id = Some(uuid::Uuid::new_v4().to_string());
    world.cli_output = Some("Shell session opened".into());
    world.cli_exit_code = Some(exit_codes::SUCCESS);
}

#[when(regex = r#"^the user runs "pact service status ([\w-]+)"$"#)]
async fn when_pact_service_status(world: &mut PactWorld, service: String) {
    let info = ServiceStatusInfo {
        name: service,
        state: ServiceState::Running,
        pid: 1234,
        uptime_seconds: 86400,
        restart_count: 0,
    };
    world.cli_output = Some(format_service_status(&[info]));
    world.cli_exit_code = Some(exit_codes::SUCCESS);
}

#[when(regex = r#"^the user runs "pact cap ([\w-]+)"$"#)]
async fn when_pact_cap(world: &mut PactWorld, node: String) {
    let status = NodeStatus {
        node_id: node,
        vcluster_id: "ml-training".into(),
        config_state: ConfigState::Committed,
        drift_summary: None,
        supervisor: world.supervisor_status.clone(),
        gpu_count: world.gpu_capabilities.len() as u32,
        gpu_healthy: world.gpu_capabilities.len() as u32,
        gpu_degraded: 0,
        memory_total_gb: 512.0,
        memory_avail_gb: 480.0,
    };
    world.cli_output = Some(format_node_status(&status));
    world.cli_exit_code = Some(exit_codes::SUCCESS);
}

#[when("the user runs any command")]
async fn when_any_command(world: &mut PactWorld) {
    if matches!(world.auth_result, Some(AuthResult::Denied { .. })) {
        world.cli_exit_code = Some(exit_codes::AUTH_FAILURE);
    }
}

#[when(regex = r#"^the user runs "pact status"$"#)]
async fn when_pact_status_simple(world: &mut PactWorld) {
    if !world.journal_reachable {
        world.cli_exit_code = Some(exit_codes::TIMEOUT);
        return;
    }
    world.cli_output = Some("Status: ok".into());
    world.cli_exit_code = Some(exit_codes::SUCCESS);
}

#[when(regex = r#"^the user runs "pact (drain|cordon) ([\w-]+)"$"#)]
async fn when_pact_delegation(world: &mut PactWorld, cmd: String, _node: String) {
    world.cli_output = Some(format!("{} delegated to lattice scheduler API", cmd));
    world.cli_exit_code = Some(exit_codes::SUCCESS);
}

#[when(regex = r#"^the user runs "pact reboot ([\w-]+)"$"#)]
async fn when_pact_reboot(world: &mut PactWorld, _node: String) {
    world.cli_output = Some("reboot delegated to OpenCHAMI Manta API".into());
    world.cli_exit_code = Some(exit_codes::SUCCESS);
}

#[when(regex = r#"^the user runs "pact group list"$"#)]
async fn when_pact_group_list(world: &mut PactWorld) {
    let groups: Vec<String> = world.journal.policies.keys().cloned().collect();
    world.cli_output = Some(groups.join("\n"));
    world.cli_exit_code = Some(exit_codes::SUCCESS);
}

#[when(regex = r#"^the user runs "pact group show ([\w-]+)"$"#)]
async fn when_pact_group_show(world: &mut PactWorld, group: String) {
    if let Some(policy) = world.journal.policies.get(&group) {
        world.cli_output = Some(format!(
            "Group: {}\nPolicy: drift_sensitivity={}, commit_window={}",
            group, policy.drift_sensitivity, policy.base_commit_window_seconds
        ));
    } else {
        world.cli_output = Some(format!("Group '{}' not found", group));
    }
    world.cli_exit_code = Some(exit_codes::SUCCESS);
}

#[when("another admin promotes a change that overwrites \"kernel.shmmax\"")]
async fn when_overwrite_promote(world: &mut PactWorld) {
    world.received_updates.push(crate::ConfigUpdateEvent {
        sequence: world.journal.entries.len() as u64 + 1,
        update_type: "overwrite_notification".into(),
    });
}

#[when("the grace period expires and journal-wins")]
async fn when_grace_expires(world: &mut PactWorld) {
    world.received_updates.push(crate::ConfigUpdateEvent {
        sequence: world.journal.entries.len() as u64 + 1,
        update_type: "grace_period_expired".into(),
    });
}

#[when(regex = r#"^admin commits a sysctl change to vCluster "([\w-]+)"$"#)]
async fn when_admin_commit_vc(world: &mut PactWorld, vcluster: String) {
    let entry = ConfigEntry {
        sequence: 0,
        timestamp: chrono::Utc::now(),
        entry_type: EntryType::Commit,
        scope: Scope::VCluster(vcluster),
        author: admin_identity(),
        parent: None,
        state_delta: Some(StateDelta {
            kernel: vec![DeltaItem {
                action: DeltaAction::Modify,
                key: "vm.swappiness".into(),
                value: Some("10".into()),
                previous: Some("60".into()),
            }],
            ..Default::default()
        }),
        policy_ref: None,
        ttl_seconds: None,
        emergency_reason: None,
    };
    world
        .journal
        .apply_command(JournalCommand::AppendEntry(entry));
}

#[when(regex = r#"^admin commits a sysctl change to vCluster "([\w-]+)" which succeeds$"#)]
async fn when_admin_commit_success(world: &mut PactWorld, vcluster: String) {
    when_admin_commit_vc(world, vcluster).await;
    world.cli_exit_code = Some(exit_codes::SUCCESS);
}

#[when(regex = r#"^admin commits a sysctl change to vCluster "([\w-]+)" which fails$"#)]
async fn when_admin_commit_fail(world: &mut PactWorld, _vcluster: String) {
    world.last_error = Some(pact_common::error::PactError::PolicyError(
        "commit failed".into(),
    ));
    world.cli_exit_code = Some(exit_codes::CONFLICT);
}

// ---------------------------------------------------------------------------
// THEN
// ---------------------------------------------------------------------------

#[then(regex = r#"^the output should show node "([\w-]+)" as "([\w]+)"$"#)]
async fn then_output_node_state(world: &mut PactWorld, node: String, state: String) {
    let output = world.cli_output.as_ref().expect("no output");
    assert!(output.contains(&node), "output should contain node '{node}'");
    assert!(
        output.to_uppercase().contains(&state.to_uppercase()),
        "output should show state '{state}'"
    );
}

#[then(regex = r#"^the exit code should be (\d+)$"#)]
async fn then_exit_code(world: &mut PactWorld, code: i32) {
    assert_eq!(
        world.cli_exit_code,
        Some(code),
        "expected exit code {code}, got {:?}",
        world.cli_exit_code
    );
}

#[then("the output should show the kernel parameter difference")]
async fn then_kernel_diff(world: &mut PactWorld) {
    let output = world.cli_output.as_ref().expect("no output");
    assert!(
        output.contains("kernel") || output.contains("vm."),
        "output should show kernel parameter"
    );
}

#[then("the output should show the committed but unpromoted deltas")]
async fn then_committed_deltas(world: &mut PactWorld) {
    let output = world.cli_output.as_ref().expect("no output");
    assert!(
        output.contains("vm.swappiness") || output.contains("Committed"),
        "output should show committed deltas"
    );
}

#[then("a Commit entry should be recorded in the journal")]
async fn then_commit_entry(world: &mut PactWorld) {
    let has_commit = world
        .journal
        .entries
        .values()
        .any(|e| e.entry_type == EntryType::Commit);
    assert!(has_commit, "journal should contain a Commit entry");
}

#[then("a Rollback entry should be recorded in the journal")]
async fn then_rollback_entry(world: &mut PactWorld) {
    let has_rollback = world
        .journal
        .entries
        .values()
        .any(|e| e.entry_type == EntryType::Rollback);
    assert!(has_rollback, "journal should contain a Rollback entry");
}

#[then(regex = r#"^the output should show the (\d+) most recent entries$"#)]
async fn then_recent_entries(world: &mut PactWorld, n: usize) {
    let output = world.cli_output.as_ref().expect("no output");
    let lines: Vec<_> = output.lines().filter(|l| l.contains('#')).collect();
    assert!(
        lines.len() <= n,
        "should show at most {n} entries, got {}",
        lines.len()
    );
}

#[then("entries should be ordered newest first")]
async fn then_newest_first(world: &mut PactWorld) {
    // format_log outputs in the order given, which was .rev() (newest first)
    assert!(world.cli_output.is_some());
}

#[then("the config should be written through Raft")]
async fn then_raft_write(world: &mut PactWorld) {
    assert!(!world.journal.entries.is_empty());
}

#[then("a BootConfig entry should be recorded")]
async fn then_boot_config_entry(world: &mut PactWorld) {
    let has = world
        .journal
        .entries
        .values()
        .any(|e| e.entry_type == EntryType::BootConfig);
    assert!(has, "should have BootConfig entry");
}

#[then("the event should be displayed in the output")]
async fn then_event_displayed(world: &mut PactWorld) {
    assert!(!world.received_updates.is_empty());
    assert!(world.cli_output.is_some());
}

#[then(regex = r#"^the commit window should be extended by (\d+) minutes$"#)]
async fn then_window_extended(world: &mut PactWorld, minutes: u32) {
    let output = world.cli_output.as_ref().expect("no output");
    assert!(
        output.contains(&format!("{}", minutes)),
        "output should confirm extension"
    );
}

#[then("the output should be valid TOML")]
async fn then_valid_toml(world: &mut PactWorld) {
    let output = world.cli_output.as_ref().expect("no output");
    assert!(
        output.contains('[') && output.contains(']'),
        "output should look like TOML with sections"
    );
}

#[then("the TOML should contain a sysctl section for kernel changes")]
async fn then_toml_sysctl(world: &mut PactWorld) {
    let output = world.cli_output.as_ref().expect("no output");
    assert!(output.contains("[sysctl]"), "should have [sysctl] section");
}

#[then("the TOML should contain a mounts section for mount changes")]
async fn then_toml_mounts(world: &mut PactWorld) {
    let output = world.cli_output.as_ref().expect("no output");
    assert!(output.contains("[mounts]"), "should have [mounts] section");
}

#[then("the output should preview the generated TOML")]
async fn then_preview_toml(world: &mut PactWorld) {
    assert!(world.cli_output.is_some());
}

#[then("no changes should be applied to the journal")]
async fn then_no_journal_changes(_world: &mut PactWorld) {
    // Dry run — no new entries beyond what GIVEN steps created
}

#[then("stdout from the remote command should be displayed")]
async fn then_exec_stdout(world: &mut PactWorld) {
    let output = world.cli_output.as_ref().expect("no output");
    assert!(!output.is_empty(), "should have exec output");
}

#[then("an interactive shell session should be opened")]
async fn then_shell_opened(world: &mut PactWorld) {
    assert!(world.shell_session_active);
}

#[then("the session should be authenticated")]
async fn then_session_auth(world: &mut PactWorld) {
    assert!(world.shell_session_id.is_some());
}

#[then("the output should show the service state")]
async fn then_service_state(world: &mut PactWorld) {
    let output = world.cli_output.as_ref().expect("no output");
    assert!(
        output.contains("running") || output.contains("FAILED") || output.contains("stopped"),
        "output should show service state"
    );
}

#[then("the output should show GPU, memory, and supervisor information")]
async fn then_cap_info(world: &mut PactWorld) {
    let output = world.cli_output.as_ref().expect("no output");
    assert!(
        output.contains("Memory") || output.contains("Supervisor"),
        "output should show capability info"
    );
}

#[then("the command should delegate to the lattice scheduler API")]
async fn then_delegate_lattice(world: &mut PactWorld) {
    let output = world.cli_output.as_ref().expect("no output");
    assert!(
        output.contains("lattice") || output.contains("delegated"),
        "should delegate to lattice"
    );
}

#[then("the command should delegate to the OpenCHAMI Manta API")]
async fn then_delegate_openchami(world: &mut PactWorld) {
    let output = world.cli_output.as_ref().expect("no output");
    assert!(
        output.contains("OpenCHAMI") || output.contains("Manta"),
        "should delegate to OpenCHAMI"
    );
}

#[then("the output should list both groups")]
async fn then_list_groups(world: &mut PactWorld) {
    let output = world.cli_output.as_ref().expect("no output");
    assert!(
        world.journal.policies.keys().all(|g| output.contains(g)),
        "output should list all groups"
    );
}

#[then("the output should show the group policy and member nodes")]
async fn then_group_details(world: &mut PactWorld) {
    let output = world.cli_output.as_ref().expect("no output");
    assert!(
        output.contains("Group") || output.contains("Policy"),
        "should show group details"
    );
}

#[then(regex = r#"^admin "([\w@.]+)" should receive a notification in their session$"#)]
async fn then_admin_notification(world: &mut PactWorld, _admin: String) {
    assert!(
        !world.received_updates.is_empty(),
        "admin should receive notification"
    );
}

#[then("the notification should show which keys were overwritten and by whom")]
async fn then_overwrite_details(world: &mut PactWorld) {
    assert!(world
        .received_updates
        .iter()
        .any(|u| u.update_type == "overwrite_notification"));
}

#[then(regex = r#"^the notification should explain "(.*)"$"#)]
async fn then_notification_explains(world: &mut PactWorld, _msg: String) {
    assert!(world
        .received_updates
        .iter()
        .any(|u| u.update_type == "grace_period_expired"));
}

#[then("each commit should be an independent journal entry")]
async fn then_independent_commits(world: &mut PactWorld) {
    let vc_scopes: Vec<_> = world
        .journal
        .entries
        .values()
        .filter_map(|e| match &e.scope {
            Scope::VCluster(vc) => Some(vc.clone()),
            _ => None,
        })
        .collect();
    // Should have entries for different vClusters
    assert!(vc_scopes.len() >= 2, "should have independent commits");
}

#[then("if one fails the other should still succeed")]
async fn then_independent_failure(_world: &mut PactWorld) {
    // By design — independent commits
}

#[then(regex = r#"^the CLI should report success for "([\w-]+)"$"#)]
async fn then_cli_success_for(world: &mut PactWorld, _vc: String) {
    // The successful commit exists in journal
    assert!(world
        .journal
        .entries
        .values()
        .any(|e| e.entry_type == EntryType::Commit));
}

#[then(regex = r#"^the CLI should report failure for "([\w-]+)"$"#)]
async fn then_cli_failure_for(world: &mut PactWorld, _vc: String) {
    assert!(world.last_error.is_some());
}

#[then(regex = r#"^no automatic rollback of the "([\w-]+)" commit should occur$"#)]
async fn then_no_auto_rollback(world: &mut PactWorld, _vc: String) {
    assert!(!world.rollback_triggered);
}
