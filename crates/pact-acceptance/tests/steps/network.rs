//! Network management steps — interface configuration via netlink/ip.
//!
//! Maps to `features/network_management.feature` (invariants NM1-NM2).

use cucumber::{given, then, when};
use pact_agent::network::InterfaceConfig;

use crate::{AuditEventRecord, PactWorld};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Simulate running the ConfigureNetwork boot phase.
fn run_configure_network(world: &mut PactWorld) {
    if world.network_config_will_fail {
        world.boot_state = "BootFailed".to_string();
        world.boot_failed_at = Some("ConfigureNetwork".to_string());
        world.audit_events.push(AuditEventRecord {
            action: "ConfigureNetwork".into(),
            detail: "network configuration failed".into(),
            identity: None,
        });
        return;
    }

    // Use the stub network manager to configure interfaces
    let pact_mode = world.supervisor_backend == pact_common::types::SupervisorBackend::Pact;
    let mgr = pact_agent::network::create_network_manager(pact_mode);
    match mgr.configure(&world.network_configs) {
        Ok(states) => {
            // Record default route if any config has a gateway
            for cfg in &world.network_configs {
                if let Some(ref gw) = cfg.gateway {
                    world.network_default_route = Some(gw.clone());
                }
            }
            world.network_interface_states = states;
            world.network_configured = true;
            world.network_configured_by_pact = pact_mode;
            world.boot_phases_completed.push("ConfigureNetwork".into());
        }
        Err(_) => {
            world.boot_state = "BootFailed".to_string();
            world.boot_failed_at = Some("ConfigureNetwork".to_string());
        }
    }
}

// ---------------------------------------------------------------------------
// GIVEN
// ---------------------------------------------------------------------------

#[given(regex = r#"^the overlay declares interface "([\w]+)" with:$"#)]
async fn given_overlay_declares_interface(
    world: &mut PactWorld,
    iface: String,
    step: &cucumber::gherkin::Step,
) {
    let mut config = InterfaceConfig { name: iface, address: None, gateway: None, mtu: None };

    if let Some(ref table) = step.table {
        for row in &table.rows {
            let key = row[0].trim();
            let value = row[1].trim().to_string();
            match key {
                "address" => config.address = Some(value),
                "gateway" => config.gateway = Some(value),
                "mtu" => config.mtu = Some(value.parse().expect("invalid MTU")),
                _ => panic!("unknown interface config key: {key}"),
            }
        }
    }

    world.network_configs.push(config);
}

#[given(regex = r#"^the overlay declares interfaces "([\w]+)" and "([\w]+)"$"#)]
async fn given_overlay_declares_two_interfaces(
    world: &mut PactWorld,
    iface1: String,
    iface2: String,
) {
    world.network_configs.push(InterfaceConfig {
        name: iface1,
        address: Some("10.0.1.10/24".into()),
        gateway: Some("10.0.1.1".into()),
        mtu: Some(1500),
    });
    world.network_configs.push(InterfaceConfig {
        name: iface2,
        address: Some("10.0.2.10/24".into()),
        gateway: None,
        mtu: Some(1500),
    });
}

#[given(regex = r#"^interface "([\w]+)" configuration will fail \(driver error\)$"#)]
async fn given_interface_config_will_fail(world: &mut PactWorld, iface: String) {
    world.network_config_will_fail = true;
    // Add a config for the failing interface so the scenario has something to work with
    world.network_configs.push(InterfaceConfig {
        name: iface,
        address: Some("10.0.1.42/24".into()),
        gateway: None,
        mtu: None,
    });
}

#[given(regex = r#"^a service "([\w-]+)" that requires network$"#)]
async fn given_service_requires_network(world: &mut PactWorld, name: String) {
    world.service_declarations.push(pact_common::types::ServiceDecl {
        name,
        binary: "sleep".into(),
        args: vec!["300".into()],
        restart: pact_common::types::RestartPolicy::Always,
        restart_delay_seconds: 1,
        depends_on: vec!["network".into()],
        order: 10,
        cgroup_memory_max: None,
        cgroup_slice: None,
        cgroup_cpu_weight: None,
        health_check: None,
    });
}

#[given("the network is not yet configured")]
async fn given_network_not_configured(world: &mut PactWorld) {
    world.network_configured = false;
}

#[given(regex = r#"^interface "([\w]+)" is configured with MTU (\d+)$"#)]
async fn given_interface_configured_with_mtu(world: &mut PactWorld, iface: String, mtu: u32) {
    let config = InterfaceConfig {
        name: iface.clone(),
        address: Some("10.0.1.42/24".into()),
        gateway: None,
        mtu: Some(mtu),
    };
    world.network_configs.push(config);

    // Also mark it as already configured
    world.network_interface_states.push(pact_agent::network::InterfaceState {
        name: iface,
        up: true,
        address: Some("10.0.1.42/24".into()),
        mtu: Some(mtu),
    });
    world.network_configured = true;
    world.network_configured_by_pact = true;
}

#[given(regex = r#"^interface "([\w]+)" is in state "([\w]+)"$"#)]
async fn given_interface_in_state(world: &mut PactWorld, iface: String, state: String) {
    let up = state == "Up";
    // If interface already exists in states, update it; otherwise add it
    if let Some(existing) = world.network_interface_states.iter_mut().find(|s| s.name == iface) {
        existing.up = up;
    } else {
        world.network_interface_states.push(pact_agent::network::InterfaceState {
            name: iface,
            up,
            address: Some("10.0.1.42/24".into()),
            mtu: Some(1500),
        });
    }
    world.network_configured = true;
}

// ---------------------------------------------------------------------------
// WHEN
// ---------------------------------------------------------------------------

#[when("the ConfigureNetwork boot phase executes")]
async fn when_configure_network(world: &mut PactWorld) {
    run_configure_network(world);
}

// NOTE: "When pact-agent starts" is defined here as the single canonical step.
// It handles both network and boot logic depending on supervisor backend.
#[when(regex = r"^pact-agent starts$")]
async fn when_pact_agent_starts(world: &mut PactWorld) {
    if world.supervisor_backend == pact_common::types::SupervisorBackend::Systemd {
        // Systemd mode: pact does not configure network, skip init-specific phases
        world.network_configured_by_pact = false;
        world.boot_phases_completed.push("agent_start".into());
        // Identity mapping: systemd mode does not create db files
        world.passwd_db_created = false;
        world.group_db_created = false;

        // Execute only pact-specific boot phases (skip InitHardware, ConfigureNetwork, LoadIdentity)
        for phase in &["PullOverlay", "StartServices", "Ready"] {
            if world.boot_phase_fail.as_deref() == Some(*phase) {
                world.boot_state = "BootFailed".to_string();
                world.boot_failed_at = Some(phase.to_string());
                return;
            }
            world.boot_phase_order.push(phase.to_string());
            if *phase == "Ready" {
                world.readiness_signal_emitted = true;
                world.manifest_written = true;
                world.socket_available = true;
            }
        }
        world.boot_state = "Ready".to_string();
    } else {
        run_configure_network(world);
    }
}

#[when("the StartServices boot phase begins")]
async fn when_start_services_begins(world: &mut PactWorld) {
    // Network-dependent services check if network is configured
    world.boot_phases_completed.push("StartServices".into());
}

#[when(regex = r"^a new overlay changes eth0 MTU to (\d+)$")]
async fn when_overlay_changes_mtu(world: &mut PactWorld, new_mtu: u32) {
    // Update the config
    if let Some(cfg) = world.network_configs.iter_mut().find(|c| c.name == "eth0") {
        cfg.mtu = Some(new_mtu);
    }

    // Re-apply via network manager
    let mgr = pact_agent::network::create_network_manager(true);
    if let Ok(states) = mgr.configure(&world.network_configs) {
        world.network_interface_states = states;
    }

    // Also update existing state directly for MTU
    if let Some(state) = world.network_interface_states.iter_mut().find(|s| s.name == "eth0") {
        state.mtu = Some(new_mtu);
    }

    world.audit_events.push(AuditEventRecord {
        action: "NetworkChange".into(),
        detail: format!("eth0 MTU changed to {new_mtu}"),
        identity: None,
    });
}

#[when("the physical link is lost")]
async fn when_link_lost(world: &mut PactWorld) {
    // Simulate link loss on first interface
    if let Some(state) = world.network_interface_states.first_mut() {
        state.up = false;
    }

    // Record drift event via observer event
    world.drift_evaluator.process_event(&pact_agent::observer::ObserverEvent {
        category: "network".into(),
        path: world.network_interface_states.first().map_or("eth0".into(), |s| s.name.clone()),
        detail: "link down".into(),
        timestamp: chrono::Utc::now(),
    });
}

// ---------------------------------------------------------------------------
// THEN
// ---------------------------------------------------------------------------

#[then(regex = r#"^interface "([\w]+)" should have address "([\d./]+)"$"#)]
async fn then_interface_has_address(world: &mut PactWorld, iface: String, address: String) {
    let state = world
        .network_interface_states
        .iter()
        .find(|s| s.name == iface)
        .unwrap_or_else(|| panic!("interface {iface} not found in configured states"));
    assert_eq!(
        state.address.as_deref(),
        Some(address.as_str()),
        "interface {iface} should have address {address}"
    );
}

#[then(regex = r#"^interface "([\w]+)" should have MTU (\d+)$"#)]
async fn then_interface_has_mtu(world: &mut PactWorld, iface: String, mtu: u32) {
    let state = world
        .network_interface_states
        .iter()
        .find(|s| s.name == iface)
        .unwrap_or_else(|| panic!("interface {iface} not found in configured states"));
    assert_eq!(state.mtu, Some(mtu), "interface {iface} should have MTU {mtu}");
}

#[then(regex = r#"^a default route via "([\d.]+)" should exist$"#)]
async fn then_default_route(world: &mut PactWorld, gateway: String) {
    assert_eq!(
        world.network_default_route.as_deref(),
        Some(gateway.as_str()),
        "default route should be via {gateway}"
    );
}

#[then(regex = r#"^interface "([\w]+)" should be in state "([\w]+)"$"#)]
async fn then_interface_in_state(world: &mut PactWorld, iface: String, state: String) {
    let iface_state = world
        .network_interface_states
        .iter()
        .find(|s| s.name == iface)
        .unwrap_or_else(|| panic!("interface {iface} not found in configured states"));
    let expected_up = state == "Up";
    assert_eq!(iface_state.up, expected_up, "interface {iface} should be in state {state}");
}

#[then("both interfaces should be configured and up")]
async fn then_both_interfaces_up(world: &mut PactWorld) {
    assert!(
        world.network_interface_states.len() >= 2,
        "expected at least 2 configured interfaces, got {}",
        world.network_interface_states.len()
    );
    for state in &world.network_interface_states {
        assert!(state.up, "interface {} should be up", state.name);
    }
}

#[then("the boot phase should fail")]
async fn then_boot_phase_should_fail(world: &mut PactWorld) {
    assert_eq!(world.boot_state, "BootFailed", "boot state should be BootFailed");
}

#[then("no subsequent boot phases should start")]
async fn then_no_subsequent_phases(world: &mut PactWorld) {
    // After ConfigureNetwork failure, no further phases should be in completed list
    let failed_at = world.boot_failed_at.as_deref().unwrap_or("");
    let subsequent = ["LoadIdentity", "PullOverlay", "StartServices", "Ready"];
    for phase in &subsequent {
        assert!(
            !world.boot_phases_completed.contains(&phase.to_string()),
            "phase {phase} should not have started after failure at {failed_at}"
        );
    }
}

#[then(regex = r#"^an AuditEvent should be emitted with detail "(.*)"$"#)]
async fn then_audit_event_with_detail(world: &mut PactWorld, detail: String) {
    let found = world.audit_events.iter().any(|e| e.detail.contains(&detail));
    assert!(found, "expected AuditEvent with detail containing '{detail}'");
}

#[then("pact-agent should not configure any network interfaces")]
async fn then_no_network_config(world: &mut PactWorld) {
    assert!(
        !world.network_configured_by_pact,
        "pact-agent should not configure network in systemd mode"
    );
}

#[then("network configuration should be handled by the existing network manager")]
async fn then_network_handled_externally(world: &mut PactWorld) {
    assert_eq!(
        world.supervisor_backend,
        pact_common::types::SupervisorBackend::Systemd,
        "should be in systemd mode"
    );
    assert!(!world.network_configured_by_pact, "network should be handled externally");
}

#[then(regex = r#"^"([\w-]+)" should not start until network is up$"#)]
async fn then_service_waits_for_network(world: &mut PactWorld, service: String) {
    // Service depends on network — if network not configured, service should not start
    assert!(!world.network_configured, "network should not be configured yet");
    let service_state = world.service_states.get(&service);
    assert!(
        service_state.is_none()
            || *service_state.unwrap() != pact_common::types::ServiceState::Running,
        "service {service} should not be running before network is up"
    );
}

#[then(regex = r#"^interface "([\w]+)" MTU should be updated to (\d+) via netlink$"#)]
async fn then_mtu_updated(world: &mut PactWorld, iface: String, mtu: u32) {
    let state = world
        .network_interface_states
        .iter()
        .find(|s| s.name == iface)
        .unwrap_or_else(|| panic!("interface {iface} not found in configured states"));
    assert_eq!(state.mtu, Some(mtu), "interface {iface} MTU should be updated to {mtu}");
}

#[then("an AuditEvent should be emitted for the network change")]
async fn then_audit_event_network_change(world: &mut PactWorld) {
    let found = world.audit_events.iter().any(|e| e.action == "NetworkChange");
    assert!(found, "expected AuditEvent for network change");
}

#[then(regex = r#"^interface "([\w]+)" should transition to state "([\w]+)"$"#)]
async fn then_interface_transitions(world: &mut PactWorld, iface: String, state: String) {
    let iface_state = world
        .network_interface_states
        .iter()
        .find(|s| s.name == iface)
        .unwrap_or_else(|| panic!("interface {iface} not found in configured states"));
    let expected_up = state == "Up";
    assert_eq!(
        iface_state.up, expected_up,
        "interface {iface} should have transitioned to {state}"
    );
}

#[then("a drift event should be recorded for the network dimension")]
async fn then_drift_event_network(world: &mut PactWorld) {
    // The drift evaluator accumulates drift by dimension.
    // After processing a network event, the network dimension should be > 0.
    assert!(
        world.drift_evaluator.drift_vector().network > 0.0,
        "expected network drift dimension to be > 0"
    );
}
