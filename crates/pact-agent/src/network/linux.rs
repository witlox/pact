//! Linux network manager — configures interfaces via `ip` commands.
//!
//! Uses `ip addr`, `ip route`, `ip link` for interface configuration.
//! Designed for static HPC network configs from the vCluster overlay.

use std::process::Command;

use tracing::{debug, error, info, warn};

use super::{InterfaceConfig, InterfaceState, NetworkManager};

/// Linux network manager using `ip` commands.
pub struct LinuxNetworkManager;

impl LinuxNetworkManager {
    /// Run an `ip` command and return stdout.
    fn ip_cmd(args: &[&str]) -> anyhow::Result<String> {
        let output = Command::new("ip").args(args).output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("ip {} failed: {}", args.join(" "), stderr.trim());
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// Bring an interface up.
    fn link_up(name: &str) -> anyhow::Result<()> {
        Self::ip_cmd(&["link", "set", name, "up"])?;
        info!(interface = %name, "interface brought up");
        Ok(())
    }

    /// Set interface MTU.
    fn set_mtu(name: &str, mtu: u32) -> anyhow::Result<()> {
        Self::ip_cmd(&["link", "set", name, "mtu", &mtu.to_string()])?;
        debug!(interface = %name, mtu = mtu, "MTU set");
        Ok(())
    }

    /// Add an IP address to an interface.
    fn add_address(name: &str, address: &str) -> anyhow::Result<()> {
        // Flush existing addresses first to be idempotent
        if let Err(e) = Self::ip_cmd(&["addr", "flush", "dev", name]) {
            warn!(interface = %name, "addr flush failed (may be fine): {e}");
        }
        Self::ip_cmd(&["addr", "add", address, "dev", name])?;
        debug!(interface = %name, address = %address, "address added");
        Ok(())
    }

    /// Add a default route via gateway.
    fn add_default_route(gateway: &str) -> anyhow::Result<()> {
        // Replace existing default route (idempotent)
        match Self::ip_cmd(&["route", "replace", "default", "via", gateway]) {
            Ok(_) => {
                debug!(gateway = %gateway, "default route set");
                Ok(())
            }
            Err(e) => {
                warn!(gateway = %gateway, "default route failed: {e}");
                Err(e)
            }
        }
    }
}

impl NetworkManager for LinuxNetworkManager {
    fn configure(&self, interfaces: &[InterfaceConfig]) -> anyhow::Result<Vec<InterfaceState>> {
        let mut states = Vec::with_capacity(interfaces.len());

        for iface in interfaces {
            info!(interface = %iface.name, "configuring network interface");

            let mut up = false;

            // Set MTU first (before adding addresses)
            if let Some(mtu) = iface.mtu {
                if let Err(e) = Self::set_mtu(&iface.name, mtu) {
                    error!(interface = %iface.name, "MTU configuration failed: {e}");
                    states.push(InterfaceState {
                        name: iface.name.clone(),
                        up: false,
                        address: None,
                        mtu: None,
                    });
                    continue;
                }
            }

            // Add address
            if let Some(ref addr) = iface.address {
                if let Err(e) = Self::add_address(&iface.name, addr) {
                    error!(interface = %iface.name, "address configuration failed: {e}");
                    states.push(InterfaceState {
                        name: iface.name.clone(),
                        up: false,
                        address: None,
                        mtu: iface.mtu,
                    });
                    continue;
                }
            }

            // Bring interface up
            match Self::link_up(&iface.name) {
                Ok(()) => up = true,
                Err(e) => {
                    error!(interface = %iface.name, "failed to bring interface up: {e}");
                }
            }

            // Add default route (if gateway specified and interface is up)
            if up {
                if let Some(ref gw) = iface.gateway {
                    if let Err(e) = Self::add_default_route(gw) {
                        warn!(interface = %iface.name, "gateway route failed (non-fatal): {e}");
                    }
                }
            }

            states.push(InterfaceState {
                name: iface.name.clone(),
                up,
                address: iface.address.clone(),
                mtu: iface.mtu,
            });

            info!(
                interface = %iface.name,
                up = up,
                address = ?iface.address,
                mtu = ?iface.mtu,
                "interface configured"
            );
        }

        Ok(states)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Linux-only tests that don't actually configure interfaces
    // (would need root/CAP_NET_ADMIN for real configuration)

    #[test]
    fn ip_cmd_handles_nonexistent_command_gracefully() {
        // This tests the error path — ip command should exist on Linux
        // but with invalid args it should return an error, not panic
        let result = LinuxNetworkManager::ip_cmd(&["nonexistent-subcommand"]);
        assert!(result.is_err());
    }
}
