//! E2E test: supervision loop lifecycle with real processes.
//!
//! Tests the full supervision loop with real process spawning,
//! crash detection, restart policy enforcement, and audit events.
//! Works on any platform (no Linux-specific features needed).

use std::sync::Arc;
use std::time::Duration;

use hpc_audit::MemoryAuditSink;
use pact_agent::supervisor::{PactSupervisor, ServiceManager, SupervisionConfig};
use pact_common::types::{RestartPolicy, ServiceDecl, ServiceState};
use tokio::time::sleep;

fn service_decl(name: &str, binary: &str, args: &[&str], policy: RestartPolicy) -> ServiceDecl {
    ServiceDecl {
        name: name.into(),
        binary: binary.into(),
        args: args.iter().map(|s| (*s).to_string()).collect(),
        restart: policy,
        restart_delay_seconds: 0,
        depends_on: vec![],
        order: 0,
        cgroup_memory_max: None,
        cgroup_slice: None,
        cgroup_cpu_weight: None,
        health_check: None,
    }
}

#[tokio::test]
async fn supervision_loop_full_lifecycle() {
    // Create supervisor with fast polling for test speed
    let sup = PactSupervisor::with_config(SupervisionConfig {
        idle_interval_ms: 50,
        active_interval_ms: 50,
    });

    // Start a service that exits immediately (simulates crash)
    let crasher = service_decl("crasher", "echo", &["crash"], RestartPolicy::Always);
    // Start a service that runs forever
    let stable = service_decl("stable", "sleep", &["300"], RestartPolicy::Never);

    sup.start_all(&[crasher.clone(), stable.clone()]).await.unwrap();

    // Verify both started
    assert_eq!(sup.status(&stable).await.unwrap().state, ServiceState::Running);

    // Start supervision loop with audit sink
    let audit = Arc::new(MemoryAuditSink::new());
    let loop_handle = sup.start_supervision_loop(
        Arc::clone(&audit),
        "e2e-node".to_string(),
        None,
        Arc::new(|| false), // idle
    );

    // Let the loop run for a bit — it should detect crasher exits and restart
    sleep(Duration::from_millis(500)).await;

    // Stable service should still be running
    assert_eq!(sup.status(&stable).await.unwrap().state, ServiceState::Running);

    // Audit sink should have crash events for the crasher
    let events = audit.events();
    assert!(!events.is_empty(), "supervision loop should have emitted audit events");
    assert!(
        events.iter().any(|e| e.action == hpc_audit::actions::SERVICE_CRASH),
        "should have crash audit events for crasher"
    );

    // Abort supervision loop
    loop_handle.abort();

    // Clean up
    sup.stop(&stable).await.unwrap();
}

#[tokio::test]
async fn supervision_loop_never_policy_stays_stopped() {
    let sup = PactSupervisor::with_config(SupervisionConfig {
        idle_interval_ms: 50,
        active_interval_ms: 50,
    });

    let oneshot = service_decl("oneshot", "echo", &["done"], RestartPolicy::Never);
    sup.start_all(std::slice::from_ref(&oneshot)).await.unwrap();

    let audit = Arc::new(MemoryAuditSink::new());
    let loop_handle = sup.start_supervision_loop(
        Arc::clone(&audit),
        "e2e-node".to_string(),
        None,
        Arc::new(|| false),
    );

    sleep(Duration::from_millis(300)).await;

    // Service should be stopped, not restarted
    let status = sup.status(&oneshot).await.unwrap();
    assert!(
        status.state == ServiceState::Stopped || status.state == ServiceState::Failed,
        "Never policy: service should be Stopped/Failed, got {:?}",
        status.state
    );

    loop_handle.abort();
}

#[tokio::test]
async fn supervision_loop_on_failure_restarts_nonzero() {
    let sup = PactSupervisor::with_config(SupervisionConfig {
        idle_interval_ms: 50,
        active_interval_ms: 50,
    });

    // `false` exits with code 1 (non-zero) — should trigger OnFailure restart
    let failing = service_decl("failing", "false", &[], RestartPolicy::OnFailure);
    sup.start_all(std::slice::from_ref(&failing)).await.unwrap();

    let audit = Arc::new(MemoryAuditSink::new());
    let loop_handle = sup.start_supervision_loop(
        Arc::clone(&audit),
        "e2e-node".to_string(),
        None,
        Arc::new(|| false),
    );

    sleep(Duration::from_millis(300)).await;

    // Should have been restarted (crash events emitted)
    let events = audit.events();
    let crash_count =
        events.iter().filter(|e| e.action == hpc_audit::actions::SERVICE_CRASH).count();
    assert!(crash_count >= 1, "OnFailure policy should have triggered at least 1 restart");

    loop_handle.abort();
}

#[tokio::test]
async fn mount_ref_manager_lifecycle() {
    use pact_agent::handoff::MountRefManager;

    let mut mgr = MountRefManager::new("/tmp/e2e-mounts", 0); // 0s hold = immediate

    // First allocation acquires
    let mp1 = mgr.acquire("pytorch-2.5.sqfs").unwrap();
    assert!(mp1.contains("pytorch-2.5"));
    assert_eq!(mgr.refcount("pytorch-2.5.sqfs"), Some(1));

    // Second allocation shares
    let mp2 = mgr.acquire("pytorch-2.5.sqfs").unwrap();
    assert_eq!(mp1, mp2); // Same mount point
    assert_eq!(mgr.refcount("pytorch-2.5.sqfs"), Some(2));

    // First releases
    mgr.release("pytorch-2.5.sqfs");
    assert_eq!(mgr.refcount("pytorch-2.5.sqfs"), Some(1));

    // Second releases — hold timer starts
    mgr.release("pytorch-2.5.sqfs");
    assert_eq!(mgr.refcount("pytorch-2.5.sqfs"), Some(0));

    // Wait for hold timer (0s) to expire
    sleep(Duration::from_millis(10)).await;

    // Check expired holds
    let expired = mgr.check_expired_holds();
    assert_eq!(expired.len(), 1);
    assert_eq!(mgr.mount_count(), 0);
}

#[tokio::test]
async fn handoff_server_namespace_lifecycle() {
    use hpc_node::namespace::{NamespaceProvider, NamespaceRequest, NamespaceType};
    use pact_agent::handoff::HandoffServer;

    let server = HandoffServer::new();

    // Not ready — should reject
    let request = NamespaceRequest {
        allocation_id: "alloc-e2e-1".into(),
        namespaces: vec![NamespaceType::Pid, NamespaceType::Net, NamespaceType::Mount],
        uenv_image: Some("pytorch-2.5.sqfs".into()),
    };
    assert!(server.create_namespaces(&request).is_err());

    // Set ready
    server.set_ready();
    assert!(server.is_ready());

    // Create namespaces
    let response = server.create_namespaces(&request).unwrap();
    assert_eq!(response.allocation_id, "alloc-e2e-1");
    assert_eq!(response.fd_types.len(), 3);
    assert!(response.uenv_mount_path.is_some());
    assert_eq!(server.active_allocation_count().await, 1);

    // Release
    server.release_namespaces("alloc-e2e-1").unwrap();
    assert_eq!(server.active_allocation_count().await, 0);
}
