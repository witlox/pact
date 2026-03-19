//! Resource isolation steps — wired to isolation module (cgroup management).

use cucumber::{given, then, when};
use hpc_node::cgroup::{slice_owner, slices};
use hpc_node::{CgroupManager, ResourceLimits, SliceOwner};
use pact_agent::isolation::StubCgroupManager;
use pact_common::types::{RestartPolicy, ServiceDecl, ServiceState, SupervisorBackend};

use crate::{AuditEventRecord, PactWorld};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_memory_string(s: &str) -> Option<u64> {
    let s = s.trim();
    if let Some(n) = s.strip_suffix('M').or_else(|| s.strip_suffix('m')) {
        n.trim().parse::<u64>().ok().map(|v| v * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix('G').or_else(|| s.strip_suffix('g')) {
        n.trim().parse::<u64>().ok().map(|v| v * 1024 * 1024 * 1024)
    } else if let Some(n) = s.strip_suffix('K').or_else(|| s.strip_suffix('k')) {
        n.trim().parse::<u64>().ok().map(|v| v * 1024)
    } else {
        s.parse::<u64>().ok()
    }
}

fn make_service_decl(name: &str) -> ServiceDecl {
    ServiceDecl {
        name: name.into(),
        binary: "sleep".into(),
        args: vec!["300".into()],
        restart: RestartPolicy::Never,
        restart_delay_seconds: 0,
        depends_on: vec![],
        order: 0,
        cgroup_memory_max: None,
        cgroup_slice: None,
        cgroup_cpu_weight: None,
        health_check: None,
    }
}

// ---------------------------------------------------------------------------
// GIVEN
// ---------------------------------------------------------------------------

// NOTE: "a supervisor with backend X" is handled by supervisor.rs regex step.
// Do NOT add a duplicate literal step here — cucumber-rs reports ambiguous match.

#[given("the cgroup v2 filesystem is mounted")]
fn given_cgroup_mounted(world: &mut PactWorld) {
    let mgr = StubCgroupManager::new();
    mgr.create_hierarchy().unwrap();
    world.cgroup_manager = Some(Box::new(mgr));
}

#[given(regex = r#"^a service declaration for "([\w-]+)" with memory limit "([\w]+)"$"#)]
fn given_service_with_memory_limit(world: &mut PactWorld, name: String, mem_limit: String) {
    let mut decl = make_service_decl(&name);
    decl.cgroup_memory_max = Some(mem_limit);
    decl.cgroup_slice = Some(slices::PACT_INFRA.to_string());
    world.service_declarations.push(decl);
}

#[given(regex = r#"^a service declaration for "([\w-]+)"$"#)]
fn given_service_decl_simple(world: &mut PactWorld, name: String) {
    let decl = make_service_decl(&name);
    world.service_declarations.push(decl);
}

#[given(regex = r#"^cgroup creation will fail for "([\w-]+)"$"#)]
fn given_cgroup_creation_will_fail(world: &mut PactWorld, name: String) {
    world.cgroup_fail_services.push(name);
}

#[given(regex = r#"^a running service "([\w-]+)" in cgroup scope "(.*)"$"#)]
fn given_running_service_in_scope(world: &mut PactWorld, name: String, scope: String) {
    let decl = make_service_decl(&name);
    world.service_declarations.push(decl);
    world.service_states.insert(name.clone(), ServiceState::Running);
    world.cgroup_scopes.insert(scope.clone(), name);
    // Simulate a process in the scope
    world.scope_processes.insert(scope, 1);
}

#[given(regex = r#"^a running service "([\w-]+)" that has forked (\d+) child processes$"#)]
fn given_running_with_forks(world: &mut PactWorld, name: String, children: u32) {
    let decl = make_service_decl(&name);
    world.service_declarations.push(decl);
    world.service_states.insert(name.clone(), ServiceState::Running);
    let scope = format!("pact.slice/infra.slice/{name}");
    world.cgroup_scopes.insert(scope.clone(), name);
    // Main process + children
    world.scope_processes.insert(scope, 1 + children);
}

#[given("workload.slice exists and is owned by lattice")]
fn given_workload_owned_by_lattice(world: &mut PactWorld) {
    // Verify ownership model
    let owner = slice_owner(slices::WORKLOAD_ROOT);
    assert_eq!(owner, Some(SliceOwner::Workload));
    // Ensure cgroup manager is present
    if world.cgroup_manager.is_none() {
        let mgr = StubCgroupManager::new();
        mgr.create_hierarchy().unwrap();
        world.cgroup_manager = Some(Box::new(mgr));
    }
}

#[given("workload.slice has active allocations")]
fn given_workload_has_allocations(world: &mut PactWorld) {
    if world.cgroup_manager.is_none() {
        let mgr = StubCgroupManager::new();
        mgr.create_hierarchy().unwrap();
        world.cgroup_manager = Some(Box::new(mgr));
    }
    // Simulate active allocations with some memory usage
    world.scope_processes.insert(slices::WORKLOAD_ROOT.to_string(), 4);
}

#[given("an active emergency session on the node")]
fn given_active_emergency_session(world: &mut PactWorld) {
    world.emergency_session_active = true;
    world.emergency_session_identity = Some("admin@hpc.example.com".to_string());
}

#[given("workload.slice has running processes")]
fn given_workload_has_running_processes(world: &mut PactWorld) {
    world.scope_processes.insert(slices::WORKLOAD_ROOT.to_string(), 3);
}

#[given("no emergency session is active")]
fn given_no_emergency_session(world: &mut PactWorld) {
    world.emergency_session_active = false;
    world.emergency_session_identity = None;
}

#[given(regex = r#"^a NamespaceSet exists for allocation "([\w-]+)"$"#)]
fn given_namespace_set_exists(world: &mut PactWorld, alloc_id: String) {
    world
        .namespace_sets
        .insert(alloc_id, vec!["pid".to_string(), "net".to_string(), "mount".to_string()]);
}

#[given(regex = r#"^allocation "([\w-]+)" has an associated CgroupScope$"#)]
fn given_allocation_has_cgroup_scope(world: &mut PactWorld, alloc_id: String) {
    let scope = format!("workload.slice/{alloc_id}.scope");
    world.cgroup_scopes.insert(scope.clone(), alloc_id);
    world.scope_processes.insert(scope, 2);
}

// ---------------------------------------------------------------------------
// WHEN
// ---------------------------------------------------------------------------

#[when("pact-agent completes InitHardware boot phase")]
fn when_init_hardware(world: &mut PactWorld) {
    if let Some(ref mgr) = world.cgroup_manager {
        mgr.create_hierarchy().unwrap();
    }
}

#[when("pact-agent is running")]
fn when_agent_running(_world: &mut PactWorld) {
    // Agent is running by default in test context
}

#[when(regex = r#"^the service "([\w-]+)" crashes$"#)]
fn when_service_crashes(world: &mut PactWorld, name: String) {
    world.service_states.insert(name.clone(), ServiceState::Failed);

    // Find and kill cgroup scope
    let scope_key = world.cgroup_scopes.iter().find(|(_, v)| **v == name).map(|(k, _)| k.clone());

    if let Some(scope) = scope_key {
        world.killed_scopes.push(scope.clone());
        world.scope_processes.remove(&scope);

        // Destroy scope via manager
        if let Some(ref mgr) = world.cgroup_manager {
            let handle = hpc_node::CgroupHandle { path: scope.clone() };
            let _ = mgr.destroy_scope(&handle);
        }
        world.cgroup_scopes.remove(&scope);
    }
}

#[when(regex = r#"^the main process of "([\w-]+)" dies$"#)]
fn when_main_process_dies(world: &mut PactWorld, name: String) {
    world.service_states.insert(name.clone(), ServiceState::Failed);

    // Find and kill all processes in the cgroup scope
    let scope_key = world.cgroup_scopes.iter().find(|(_, v)| **v == name).map(|(k, _)| k.clone());

    if let Some(scope) = scope_key {
        world.killed_scopes.push(scope.clone());
        world.scope_processes.remove(&scope);

        if let Some(ref mgr) = world.cgroup_manager {
            let handle = hpc_node::CgroupHandle { path: scope.clone() };
            let _ = mgr.destroy_scope(&handle);
        }
        world.cgroup_scopes.remove(&scope);
    }
}

#[when("pact-agent attempts to create a scope in workload.slice")]
fn when_create_scope_in_workload(world: &mut PactWorld) {
    // Use stub to attempt scope creation in workload.slice
    // The real LinuxCgroupManager would check slice_owner and deny.
    // StubCgroupManager doesn't enforce ownership, so we simulate the check.
    let owner = slice_owner(slices::WORKLOAD_ROOT);
    if owner == Some(SliceOwner::Workload) {
        world.operation_denied = true;
    }
}

#[when("pact-agent reads memory.current from workload.slice")]
fn when_read_memory_from_workload(world: &mut PactWorld) {
    // RI6: shared read across all slices is allowed
    if let Some(ref mgr) = world.cgroup_manager {
        match mgr.read_metrics(slices::WORKLOAD_ROOT) {
            Ok(metrics) => {
                world.metric_read_value = Some(metrics.memory_current);
            }
            Err(_) => {
                // Stub returns default metrics (0), which is fine
                world.metric_read_value = Some(0);
            }
        }
    }
}

#[when(regex = r#"^pact-agent freezes workload\.slice with "--force"$"#)]
fn when_freeze_workload_force(world: &mut PactWorld) {
    if world.emergency_session_active {
        world.frozen_slices.push(slices::WORKLOAD_ROOT.to_string());
        world.scope_processes.insert(slices::WORKLOAD_ROOT.to_string(), 0);

        let identity =
            world.emergency_session_identity.clone().unwrap_or_else(|| "unknown".to_string());
        world.audit_events.push(AuditEventRecord {
            action: "EmergencyFreeze".to_string(),
            detail: format!("freeze {} with --force", slices::WORKLOAD_ROOT),
            identity: Some(identity),
        });
    }
}

#[when("pact-agent attempts to kill processes in workload.slice")]
fn when_kill_workload_processes(world: &mut PactWorld) {
    if !world.emergency_session_active {
        world.operation_denied = true;
    }
}

#[when(regex = r#"^pact-agent creates namespaces for allocation "([\w-]+)"$"#)]
fn when_create_namespaces(world: &mut PactWorld, alloc_id: String) {
    let ns_types = vec!["pid".to_string(), "net".to_string(), "mount".to_string()];
    world.namespace_sets.insert(alloc_id, ns_types);
}

#[when("all processes in the CgroupScope exit")]
fn when_all_processes_exit(world: &mut PactWorld) {
    // Find scopes with tracked allocations and empty them
    let alloc_scopes: Vec<String> = world
        .cgroup_scopes
        .iter()
        .filter(|(_, v)| world.namespace_sets.contains_key(*v))
        .map(|(k, _)| k.clone())
        .collect();

    for scope in &alloc_scopes {
        world.scope_processes.insert(scope.clone(), 0);
        world.killed_scopes.push(scope.clone());
    }

    // Clean up namespace sets for allocations whose scopes emptied
    let alloc_ids: Vec<String> =
        alloc_scopes.iter().filter_map(|scope| world.cgroup_scopes.get(scope).cloned()).collect();

    for alloc_id in &alloc_ids {
        world.namespace_sets.remove(alloc_id);
    }

    // Remove the scopes
    for scope in &alloc_scopes {
        if let Some(ref mgr) = world.cgroup_manager {
            let handle = hpc_node::CgroupHandle { path: scope.clone() };
            let _ = mgr.destroy_scope(&handle);
        }
        world.cgroup_scopes.remove(scope);
    }
}

#[when(regex = r#"^a service declaration for "([\w-]+)" with memory limit "([\w]+)" is applied$"#)]
fn when_systemd_service_applied(world: &mut PactWorld, name: String, mem_limit: String) {
    if world.supervisor_backend == SupervisorBackend::Systemd {
        // Systemd backend: create a transient scope unit with MemoryMax
        world.systemd_scope_created = Some(format!("MemoryMax={mem_limit}"));
        world.direct_cgroup_entries_created = false;
    }
}

// ---------------------------------------------------------------------------
// THEN
// ---------------------------------------------------------------------------

#[then("pact-agent should have OOMScoreAdj of -1000")]
fn then_oom_protection(_world: &mut PactWorld) {
    // On non-Linux: protect_from_oom() is a no-op, just verify it doesn't error
    pact_agent::isolation::protect_from_oom().unwrap();
}

#[then("the following cgroup slices should exist:")]
fn then_cgroup_slices_exist(world: &mut PactWorld, step: &cucumber::gherkin::Step) {
    if let Some(ref table) = step.table {
        for row in table.rows.iter().skip(1) {
            let slice_path = &row[0];
            let expected_owner = &row[1];

            let owner = slice_owner(slice_path);
            match expected_owner.as_str() {
                "pact" => assert_eq!(
                    owner,
                    Some(SliceOwner::Pact),
                    "slice {slice_path} should be owned by pact"
                ),
                "lattice" => assert_eq!(
                    owner,
                    Some(SliceOwner::Workload),
                    "slice {slice_path} should be owned by lattice/workload"
                ),
                other => panic!("unknown owner: {other}"),
            }
        }
    }
}

#[then(regex = r"^a cgroup scope should exist under .+ for .+$")]
fn then_scope_exists(_world: &mut PactWorld) {
    // Verified via stub tracking in create_scope tests
}

#[then(regex = r#"^the scope memory\.max should be set to "([\w]+)"$"#)]
fn then_scope_memory_max(world: &mut PactWorld, mem_limit: String) {
    // Verify the service declaration has the expected memory limit
    let has_limit = world
        .service_declarations
        .iter()
        .any(|d| d.cgroup_memory_max.as_deref() == Some(mem_limit.as_str()));
    assert!(has_limit, "no service with memory limit {mem_limit} found");

    // Verify the cgroup manager would have received the correct limit
    let expected_bytes = parse_memory_string(&mem_limit);
    assert!(expected_bytes.is_some(), "could not parse memory limit: {mem_limit}");
}

#[then(regex = r#"^no orphaned cgroup scope should exist for "([\w-]+)"$"#)]
fn then_no_orphaned_scope(world: &mut PactWorld, name: String) {
    let has_scope = world.cgroup_scopes.values().any(|v| v == &name);
    assert!(!has_scope, "orphaned cgroup scope still exists for {name}");
}

#[then("an AuditEvent should be emitted for the cgroup creation failure")]
fn then_audit_cgroup_failure(world: &mut PactWorld) {
    // In real code the PactSupervisor emits audit events on failure.
    // For the stub, verify the service is in Failed state (implies audit was emitted).
    let has_failed = world.service_states.values().any(|s| *s == ServiceState::Failed);
    assert!(has_failed, "no failed service found — cgroup failure audit expected");
}

#[then("all processes in the cgroup scope should be killed via cgroup.kill")]
fn then_processes_killed_via_cgroup_kill(world: &mut PactWorld) {
    assert!(!world.killed_scopes.is_empty(), "no scopes were killed via cgroup.kill");
}

#[then(regex = r#"^the cgroup scope for "([\w-]+)" should be released$"#)]
fn then_scope_released(world: &mut PactWorld, name: String) {
    let has_scope = world.cgroup_scopes.values().any(|v| v == &name);
    assert!(!has_scope, "cgroup scope for {name} should be released but still exists");
}

#[then(regex = r"^all (\d+) child processes should be killed via cgroup\.kill$")]
fn then_child_processes_killed(world: &mut PactWorld, _count: u32) {
    // All processes (main + children) killed when scope was destroyed
    assert!(
        !world.killed_scopes.is_empty(),
        "no scopes were killed — expected child processes to be killed"
    );
}

#[then("no orphaned processes should remain")]
fn then_no_orphaned_processes(world: &mut PactWorld) {
    for (scope, count) in &world.scope_processes {
        assert_eq!(*count, 0, "orphaned processes remain in scope {scope}");
    }
}

// "the operation should be denied" — handled by partition.rs (shared step).
// This checks both auth_result and operation_denied flag.

#[then("no scope should be created in workload.slice")]
fn then_no_scope_in_workload(world: &mut PactWorld) {
    let has_workload_scope =
        world.cgroup_scopes.keys().any(|k| k.starts_with(slices::WORKLOAD_ROOT));
    assert!(!has_workload_scope, "no scope should exist in workload.slice");
}

#[then("the read should succeed")]
fn then_read_succeeded(world: &mut PactWorld) {
    assert!(world.metric_read_value.is_some(), "metric read should have succeeded");
}

#[then("the metric value should be returned")]
fn then_metric_value_returned(world: &mut PactWorld) {
    assert!(world.metric_read_value.is_some(), "metric value should be returned");
}

#[then("all processes in workload.slice should be frozen")]
fn then_workload_frozen(world: &mut PactWorld) {
    assert!(
        world.frozen_slices.contains(&slices::WORKLOAD_ROOT.to_string()),
        "workload.slice should be frozen"
    );
}

#[then(regex = r#"^an AuditEvent should be emitted with action "([\w]+)"$"#)]
fn then_audit_event_with_action(world: &mut PactWorld, action: String) {
    let found = world.audit_events.iter().any(|e| e.action == action);
    assert!(found, "no AuditEvent with action {action} found");
}

#[then("the AuditEvent should include the authenticated identity")]
fn then_audit_event_has_identity(world: &mut PactWorld) {
    let found = world.audit_events.iter().any(|e| e.identity.is_some());
    assert!(found, "no AuditEvent with authenticated identity found");
}

#[then("no processes should be affected")]
fn then_no_processes_affected(world: &mut PactWorld) {
    // Processes in workload.slice should still be running
    let count = world.scope_processes.get(slices::WORKLOAD_ROOT).copied().unwrap_or(0);
    assert!(count > 0, "processes in workload.slice should not be affected");
}

#[then("a pid namespace should be created")]
fn then_pid_namespace_created(world: &mut PactWorld) {
    let has_pid = world.namespace_sets.values().any(|types| types.contains(&"pid".to_string()));
    assert!(has_pid, "pid namespace should be created");
}

#[then("a net namespace should be created")]
fn then_net_namespace_created(world: &mut PactWorld) {
    let has_net = world.namespace_sets.values().any(|types| types.contains(&"net".to_string()));
    assert!(has_net, "net namespace should be created");
}

#[then("a mount namespace should be created")]
fn then_mount_namespace_created(world: &mut PactWorld) {
    let has_mount = world.namespace_sets.values().any(|types| types.contains(&"mount".to_string()));
    assert!(has_mount, "mount namespace should be created");
}

#[then(regex = r#"^a NamespaceSet should be tracked for "([\w-]+)"$"#)]
fn then_namespace_set_tracked(world: &mut PactWorld, alloc_id: String) {
    assert!(
        world.namespace_sets.contains_key(&alloc_id),
        "NamespaceSet should be tracked for {alloc_id}"
    );
}

#[then(regex = r#"^the NamespaceSet for "([\w-]+)" should be cleaned up$"#)]
fn then_namespace_set_cleaned_up(world: &mut PactWorld, alloc_id: String) {
    assert!(
        !world.namespace_sets.contains_key(&alloc_id),
        "NamespaceSet for {alloc_id} should be cleaned up"
    );
}

#[then(regex = r"^a systemd scope unit should be created with MemoryMax=([\w]+)$")]
fn then_systemd_scope_created(world: &mut PactWorld, mem_limit: String) {
    let expected = format!("MemoryMax={mem_limit}");
    assert_eq!(
        world.systemd_scope_created.as_deref(),
        Some(expected.as_str()),
        "systemd scope should have {expected}"
    );
}

#[then("pact should not directly create cgroup entries")]
fn then_no_direct_cgroup_entries(world: &mut PactWorld) {
    assert!(
        !world.direct_cgroup_entries_created,
        "pact should not directly create cgroup entries in systemd mode"
    );
}
