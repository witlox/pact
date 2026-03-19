//! Network interface detection backend.
//!
//! Detects network interfaces from `/sys/class/net/` on Linux.
//! Determines fabric type (Slingshot vs Ethernet) from the driver symlink.

use async_trait::async_trait;
use pact_common::types::{InterfaceOperState, NetworkFabric, NetworkInterface};

/// Trait for network detection backends.
#[async_trait]
pub trait NetworkBackend: Send + Sync {
    /// Detect network interfaces and return their capabilities.
    async fn detect(&self) -> anyhow::Result<Vec<NetworkInterface>>;
}

/// Linux network backend — reads `/sys/class/net/` and sysfs attributes.
pub struct LinuxNetworkBackend;

impl LinuxNetworkBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LinuxNetworkBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NetworkBackend for LinuxNetworkBackend {
    async fn detect(&self) -> anyhow::Result<Vec<NetworkInterface>> {
        let mut interfaces = Vec::new();

        let net_dir = std::path::Path::new("/sys/class/net");
        let entries = match std::fs::read_dir(net_dir) {
            Ok(e) => e,
            Err(_) => return Ok(vec![]), // non-Linux or unreadable
        };

        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();

            // Skip loopback
            if name == "lo" {
                continue;
            }

            // Skip virtual interfaces (no /sys/class/net/*/device symlink)
            let device_path = net_dir.join(&name).join("device");
            if !device_path.exists() {
                continue;
            }

            let speed_mbps = read_sysfs_speed(net_dir, &name);
            let state = read_sysfs_operstate(net_dir, &name);
            let mac = read_sysfs_file(net_dir, &name, "address");
            let fabric = detect_fabric(net_dir, &name);

            interfaces.push(NetworkInterface { name, fabric, speed_mbps, state, mac, ipv4: None });
        }

        Ok(interfaces)
    }
}

/// Read the interface speed from sysfs. Returns 0 if unreadable or negative.
fn read_sysfs_speed(net_dir: &std::path::Path, iface: &str) -> u64 {
    let path = net_dir.join(iface).join("speed");
    match std::fs::read_to_string(&path) {
        Ok(content) => parse_speed(&content),
        Err(_) => 0,
    }
}

/// Parse speed string from sysfs. Negative values (e.g., -1 for unknown) become 0.
pub(crate) fn parse_speed(content: &str) -> u64 {
    let trimmed = content.trim();
    match trimmed.parse::<i64>() {
        Ok(v) if v > 0 => v as u64,
        _ => 0,
    }
}

/// Read the interface operational state from sysfs.
fn read_sysfs_operstate(net_dir: &std::path::Path, iface: &str) -> InterfaceOperState {
    let content = read_sysfs_file(net_dir, iface, "operstate");
    parse_operstate(&content)
}

/// Parse operstate string from sysfs.
pub(crate) fn parse_operstate(content: &str) -> InterfaceOperState {
    match content.trim() {
        "up" => InterfaceOperState::Up,
        _ => InterfaceOperState::Down,
    }
}

/// Read a sysfs file for a network interface, returning trimmed content or empty string.
fn read_sysfs_file(net_dir: &std::path::Path, iface: &str, attr: &str) -> String {
    let path = net_dir.join(iface).join(attr);
    std::fs::read_to_string(&path).map(|s| s.trim().to_string()).unwrap_or_default()
}

/// Detect network fabric type from the driver symlink.
fn detect_fabric(net_dir: &std::path::Path, iface: &str) -> NetworkFabric {
    let driver_link = net_dir.join(iface).join("device").join("driver");
    match std::fs::read_link(&driver_link) {
        Ok(target) => {
            let driver_name =
                target.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            parse_fabric(&driver_name)
        }
        Err(_) => NetworkFabric::Unknown,
    }
}

/// Parse driver name to determine fabric type.
pub(crate) fn parse_fabric(driver_name: &str) -> NetworkFabric {
    if driver_name.contains("cxi") {
        NetworkFabric::Slingshot
    } else {
        NetworkFabric::Ethernet
    }
}

/// Mock network backend for development/testing.
pub struct MockNetworkBackend {
    pub interfaces: Vec<NetworkInterface>,
}

impl MockNetworkBackend {
    pub fn new() -> Self {
        Self { interfaces: vec![] }
    }
}

impl Default for MockNetworkBackend {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NetworkBackend for MockNetworkBackend {
    async fn detect(&self) -> anyhow::Result<Vec<NetworkInterface>> {
        Ok(self.interfaces.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_speed tests ---

    #[test]
    fn parse_speed_normal() {
        assert_eq!(parse_speed("1000\n"), 1000);
    }

    #[test]
    fn parse_speed_negative_returns_zero() {
        assert_eq!(parse_speed("-1\n"), 0);
    }

    #[test]
    fn parse_speed_zero() {
        assert_eq!(parse_speed("0\n"), 0);
    }

    #[test]
    fn parse_speed_high_bandwidth() {
        assert_eq!(parse_speed("200000\n"), 200_000);
    }

    #[test]
    fn parse_speed_empty() {
        assert_eq!(parse_speed(""), 0);
    }

    #[test]
    fn parse_speed_garbage() {
        assert_eq!(parse_speed("not_a_number\n"), 0);
    }

    // --- parse_operstate tests ---

    #[test]
    fn parse_operstate_up() {
        assert_eq!(parse_operstate("up\n"), InterfaceOperState::Up);
    }

    #[test]
    fn parse_operstate_down() {
        assert_eq!(parse_operstate("down\n"), InterfaceOperState::Down);
    }

    #[test]
    fn parse_operstate_unknown_is_down() {
        assert_eq!(parse_operstate("unknown\n"), InterfaceOperState::Down);
    }

    #[test]
    fn parse_operstate_empty_is_down() {
        assert_eq!(parse_operstate(""), InterfaceOperState::Down);
    }

    // --- parse_fabric tests ---

    #[test]
    fn parse_fabric_cxi_is_slingshot() {
        assert_eq!(parse_fabric("cxi"), NetworkFabric::Slingshot);
    }

    #[test]
    fn parse_fabric_cxi_core_is_slingshot() {
        assert_eq!(parse_fabric("cxi_core"), NetworkFabric::Slingshot);
    }

    #[test]
    fn parse_fabric_mlx5_core_is_ethernet() {
        assert_eq!(parse_fabric("mlx5_core"), NetworkFabric::Ethernet);
    }

    #[test]
    fn parse_fabric_e1000e_is_ethernet() {
        assert_eq!(parse_fabric("e1000e"), NetworkFabric::Ethernet);
    }

    #[test]
    fn parse_fabric_empty_is_ethernet() {
        assert_eq!(parse_fabric(""), NetworkFabric::Ethernet);
    }

    // --- MockNetworkBackend tests ---

    #[tokio::test]
    async fn mock_backend_returns_configured_interfaces() {
        let backend = MockNetworkBackend {
            interfaces: vec![
                NetworkInterface {
                    name: "eth0".into(),
                    fabric: NetworkFabric::Ethernet,
                    speed_mbps: 1000,
                    state: InterfaceOperState::Up,
                    mac: "aa:bb:cc:dd:ee:ff".into(),
                    ipv4: Some("10.0.0.1".into()),
                },
                NetworkInterface {
                    name: "cxi0".into(),
                    fabric: NetworkFabric::Slingshot,
                    speed_mbps: 200_000,
                    state: InterfaceOperState::Up,
                    mac: "00:11:22:33:44:55".into(),
                    ipv4: None,
                },
            ],
        };

        let interfaces = backend.detect().await.unwrap();
        assert_eq!(interfaces.len(), 2);
        assert_eq!(interfaces[0].name, "eth0");
        assert_eq!(interfaces[0].fabric, NetworkFabric::Ethernet);
        assert_eq!(interfaces[1].name, "cxi0");
        assert_eq!(interfaces[1].fabric, NetworkFabric::Slingshot);
    }

    #[tokio::test]
    async fn mock_backend_empty() {
        let backend = MockNetworkBackend::new();
        let interfaces = backend.detect().await.unwrap();
        assert!(interfaces.is_empty());
    }
}
