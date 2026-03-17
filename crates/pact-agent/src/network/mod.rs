//! Network management — interface configuration.
//!
//! Configures network interfaces when pact-agent is init (PactSupervisor mode).
//! In systemd mode, network configuration is delegated to wickedd/NetworkManager.
//!
//! # Implementation
//!
//! Uses `ip` commands via `std::process::Command` for interface configuration.
//! This is pragmatic for the known use case (static IPs from overlay, 1-4 interfaces).
//! A direct netlink implementation can replace this later if needed.
//!
//! # Invariants enforced
//!
//! - NM1: Netlink/ip only in PactSupervisor mode
//! - NM2: Network before services (enforced by boot phase ordering)

#[cfg(target_os = "linux")]
mod linux;
mod stub;

use serde::{Deserialize, Serialize};

/// Network interface configuration from the vCluster overlay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceConfig {
    /// Interface name (e.g., "eth0", "hsn0").
    pub name: String,
    /// IP address with prefix length (e.g., "10.0.1.42/24").
    pub address: Option<String>,
    /// Default gateway (e.g., "10.0.1.1").
    pub gateway: Option<String>,
    /// MTU (e.g., 9000 for jumbo frames).
    pub mtu: Option<u32>,
}

/// Result of configuring an interface.
#[derive(Debug, Clone)]
pub struct InterfaceState {
    pub name: String,
    pub up: bool,
    pub address: Option<String>,
    pub mtu: Option<u32>,
}

/// Trait for network configuration.
///
/// PactSupervisor mode: implements via ip commands / netlink.
/// SystemdBackend mode: no-op (wickedd handles it).
pub trait NetworkManager: Send + Sync {
    /// Configure interfaces from overlay declarations.
    /// Returns the state of each configured interface.
    fn configure(&self, interfaces: &[InterfaceConfig]) -> anyhow::Result<Vec<InterfaceState>>;
}

/// Create the appropriate network manager for the current platform and mode.
#[must_use]
pub fn create_network_manager(pact_mode: bool) -> Box<dyn NetworkManager> {
    if !pact_mode {
        return Box::new(stub::StubNetworkManager);
    }

    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxNetworkManager)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Box::new(stub::StubNetworkManager)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interface_config_serialization() {
        let cfg = InterfaceConfig {
            name: "eth0".into(),
            address: Some("10.0.1.42/24".into()),
            gateway: Some("10.0.1.1".into()),
            mtu: Some(9000),
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let deser: InterfaceConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.name, "eth0");
        assert_eq!(deser.mtu, Some(9000));
    }

    #[test]
    fn stub_manager_succeeds() {
        let mgr = stub::StubNetworkManager;
        let configs = vec![InterfaceConfig {
            name: "eth0".into(),
            address: Some("10.0.1.1/24".into()),
            gateway: None,
            mtu: Some(1500),
        }];
        let states = mgr.configure(&configs).unwrap();
        assert_eq!(states.len(), 1);
        assert!(states[0].up);
    }

    #[test]
    fn systemd_mode_creates_stub() {
        let mgr = create_network_manager(false);
        let states = mgr.configure(&[]).unwrap();
        assert!(states.is_empty());
    }
}
