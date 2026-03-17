//! Stub network manager for macOS development and systemd mode.

use tracing::info;

use super::{InterfaceConfig, InterfaceState, NetworkManager};

/// No-op network manager — logs operations, always succeeds.
pub struct StubNetworkManager;

impl NetworkManager for StubNetworkManager {
    fn configure(&self, interfaces: &[InterfaceConfig]) -> anyhow::Result<Vec<InterfaceState>> {
        let mut states = Vec::with_capacity(interfaces.len());
        for iface in interfaces {
            info!(
                interface = %iface.name,
                address = ?iface.address,
                mtu = ?iface.mtu,
                "stub: configuring interface (no-op)"
            );
            states.push(InterfaceState {
                name: iface.name.clone(),
                up: true,
                address: iface.address.clone(),
                mtu: iface.mtu,
            });
        }
        Ok(states)
    }
}
