//! CLI commands for node enrollment, assignment, and management.
//!
//! Maps to EnrollmentService gRPC RPCs.

use serde::Deserialize;
use tonic::transport::Channel;
use tonic::Request;

use pact_common::proto::enrollment::enrollment_service_client::EnrollmentServiceClient;
use pact_common::proto::enrollment::{
    AssignNodeRequest, DecommissionNodeRequest, HardwareIdentity, InspectNodeRequest,
    ListNodesRequest, MoveNodeRequest, RegisterNodeRequest, UnassignNodeRequest,
};

// --- SMD types for OpenCHAMI node import ---

/// A component from the SMD `/hsm/v2/State/Components` response.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SmdComponent {
    /// HSM component ID (xname), e.g. "x1000c0s0b0n0".
    #[serde(rename = "ID")]
    pub id: String,
    /// Component state, e.g. "Ready", "Off".
    #[serde(default)]
    pub state: String,
    /// Component type, e.g. "Node".
    #[serde(rename = "Type", default)]
    pub component_type: String,
    /// Role, e.g. "Compute", "Service".
    #[serde(default)]
    pub role: String,
}

/// Top-level response from SMD `/hsm/v2/State/Components`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SmdComponentsResponse {
    pub components: Vec<SmdComponent>,
}

/// An ethernet interface from SMD `/hsm/v2/Inventory/EthernetInterfaces`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SmdEthernetInterface {
    /// Interface ID, e.g. "x1000c0s0b0n0:eth0".
    #[serde(rename = "ID", default)]
    pub id: String,
    /// MAC address.
    #[serde(rename = "MACAddress", default)]
    pub mac_address: String,
    /// Component ID this interface belongs to.
    #[serde(rename = "ComponentID", default)]
    pub component_id: String,
}

/// Top-level response from SMD `/hsm/v2/Inventory/EthernetInterfaces`.
#[derive(Debug, Deserialize)]
pub struct SmdEthernetInterfacesResponse(pub Vec<SmdEthernetInterface>);

/// Enroll (register) a node by ID with hardware identity.
pub async fn enroll(
    channel: &Channel,
    token: &str,
    node_id: &str,
    mac: &str,
    bmc_serial: Option<&str>,
) -> anyhow::Result<String> {
    let mut client = EnrollmentServiceClient::new(channel.clone());
    let mut request = Request::new(RegisterNodeRequest {
        node_id: node_id.to_string(),
        hardware_identity: Some(HardwareIdentity {
            mac_address: mac.to_string(),
            bmc_serial: bmc_serial.unwrap_or_default().to_string(),
            extra: std::collections::HashMap::new(),
        }),
    });
    request.metadata_mut().insert("authorization", format!("Bearer {token}").parse().unwrap());

    let resp = client.register_node(request).await?.into_inner();
    Ok(format!("Enrolled node {} — state: {}", resp.node_id, resp.enrollment_state))
}

/// Decommission a node.
pub async fn decommission(
    channel: &Channel,
    token: &str,
    node_id: &str,
    force: bool,
) -> anyhow::Result<String> {
    let mut client = EnrollmentServiceClient::new(channel.clone());
    let mut request = Request::new(DecommissionNodeRequest { node_id: node_id.to_string(), force });
    request.metadata_mut().insert("authorization", format!("Bearer {token}").parse().unwrap());

    let resp = client.decommission_node(request).await?.into_inner();
    let mut output =
        format!("Decommissioned node {} — state: {}", resp.node_id, resp.enrollment_state);
    if !resp.cert_serial_revoked.is_empty() {
        output.push_str(&format!("\n  Certificate revoked: {}", resp.cert_serial_revoked));
    }
    if resp.sessions_terminated > 0 {
        output.push_str(&format!("\n  Sessions terminated: {}", resp.sessions_terminated));
    }
    Ok(output)
}

/// Assign a node to a vCluster.
pub async fn assign(
    channel: &Channel,
    token: &str,
    node_id: &str,
    vcluster_id: &str,
) -> anyhow::Result<String> {
    let mut client = EnrollmentServiceClient::new(channel.clone());
    let mut request = Request::new(AssignNodeRequest {
        node_id: node_id.to_string(),
        vcluster_id: vcluster_id.to_string(),
    });
    request.metadata_mut().insert("authorization", format!("Bearer {token}").parse().unwrap());

    let resp = client.assign_node(request).await?.into_inner();
    Ok(format!("Assigned node {} to vCluster {}", resp.node_id, resp.vcluster_id))
}

/// Unassign a node from its vCluster.
pub async fn unassign(channel: &Channel, token: &str, node_id: &str) -> anyhow::Result<String> {
    let mut client = EnrollmentServiceClient::new(channel.clone());
    let mut request = Request::new(UnassignNodeRequest { node_id: node_id.to_string() });
    request.metadata_mut().insert("authorization", format!("Bearer {token}").parse().unwrap());

    let resp = client.unassign_node(request).await?.into_inner();
    Ok(format!("Unassigned node {} from vCluster", resp.node_id))
}

/// Move a node between vClusters.
pub async fn move_node(
    channel: &Channel,
    token: &str,
    node_id: &str,
    to_vcluster: &str,
) -> anyhow::Result<String> {
    let mut client = EnrollmentServiceClient::new(channel.clone());
    let mut request = Request::new(MoveNodeRequest {
        node_id: node_id.to_string(),
        to_vcluster_id: to_vcluster.to_string(),
    });
    request.metadata_mut().insert("authorization", format!("Bearer {token}").parse().unwrap());

    let resp = client.move_node(request).await?.into_inner();
    Ok(format!(
        "Moved node {} from {} to {}",
        resp.node_id, resp.from_vcluster_id, resp.to_vcluster_id
    ))
}

/// List enrolled nodes.
pub async fn list(
    channel: &Channel,
    token: &str,
    state_filter: Option<&str>,
    vcluster_filter: Option<&str>,
    unassigned_only: bool,
) -> anyhow::Result<String> {
    let mut client = EnrollmentServiceClient::new(channel.clone());
    let mut request = Request::new(ListNodesRequest {
        state_filter: state_filter.unwrap_or_default().to_string(),
        vcluster_filter: vcluster_filter.unwrap_or_default().to_string(),
        unassigned_only,
    });
    request.metadata_mut().insert("authorization", format!("Bearer {token}").parse().unwrap());

    let resp = client.list_nodes(request).await?.into_inner();
    if resp.nodes.is_empty() {
        return Ok("No nodes found.".to_string());
    }

    let mut output = format!(
        "{:<20} {:<12} {:<20} {:<20} {}\n",
        "NODE ID", "STATE", "VCLUSTER", "LAST SEEN", "MAC"
    );
    output.push_str(&"-".repeat(90));
    output.push('\n');

    for node in &resp.nodes {
        output.push_str(&format!(
            "{:<20} {:<12} {:<20} {:<20} {}\n",
            node.node_id,
            node.enrollment_state,
            if node.vcluster_id.is_empty() { "(none)" } else { &node.vcluster_id },
            if node.last_seen.is_empty() { "-" } else { &node.last_seen },
            node.mac_address,
        ));
    }
    Ok(output)
}

/// Inspect a single node's enrollment details.
pub async fn inspect(channel: &Channel, token: &str, node_id: &str) -> anyhow::Result<String> {
    let mut client = EnrollmentServiceClient::new(channel.clone());
    let mut request = Request::new(InspectNodeRequest { node_id: node_id.to_string() });
    request.metadata_mut().insert("authorization", format!("Bearer {token}").parse().unwrap());

    let resp = client.inspect_node(request).await?.into_inner();
    let hw = resp.hardware_identity.as_ref();

    let mut output = format!("Node: {}\n", resp.node_id);
    output.push_str(&format!("  Domain:            {}\n", resp.domain_id));
    output.push_str(&format!("  Enrollment State:  {}\n", resp.enrollment_state));
    if let Some(hw) = hw {
        output.push_str(&format!("  MAC Address:       {}\n", hw.mac_address));
        if !hw.bmc_serial.is_empty() {
            output.push_str(&format!("  BMC Serial:        {}\n", hw.bmc_serial));
        }
    }
    output.push_str(&format!(
        "  vCluster:          {}\n",
        if resp.vcluster_id.is_empty() { "(none)" } else { &resp.vcluster_id }
    ));
    if !resp.cert_serial.is_empty() {
        output.push_str(&format!("  Certificate:       {}\n", resp.cert_serial));
        output.push_str(&format!("  Cert Expires:      {}\n", resp.cert_expires_at));
    }
    if !resp.last_seen.is_empty() {
        output.push_str(&format!("  Last Seen:         {}\n", resp.last_seen));
    }
    output.push_str(&format!("  Enrolled At:       {}\n", resp.enrolled_at));
    output.push_str(&format!("  Enrolled By:       {}\n", resp.enrolled_by));
    if resp.active_sessions > 0 {
        output.push_str(&format!("  Active Sessions:   {}\n", resp.active_sessions));
    }
    Ok(output)
}

// --- SMD import helpers ---

/// Query SMD for node components, optionally filtered by group/role.
pub async fn query_smd_components(
    smd_url: &str,
    smd_token: Option<&str>,
    group: Option<&str>,
    timeout_secs: u64,
) -> anyhow::Result<Vec<SmdComponent>> {
    let client =
        reqwest::Client::builder().timeout(std::time::Duration::from_secs(timeout_secs)).build()?;

    let url = format!("{}/hsm/v2/State/Components?type=Node", smd_url.trim_end_matches('/'));
    let mut req = client.get(&url);
    if let Some(token) = smd_token {
        req = req.header("Authorization", format!("Bearer {token}"));
    }

    let resp = req.send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("SMD components query failed: HTTP {}", resp.status());
    }

    let body: SmdComponentsResponse = resp.json().await?;
    let components = if let Some(group_filter) = group {
        body.components.into_iter().filter(|c| c.role.eq_ignore_ascii_case(group_filter)).collect()
    } else {
        body.components
    };

    Ok(components)
}

/// Query SMD for ethernet interfaces of a specific component.
pub async fn query_smd_ethernet(
    smd_url: &str,
    smd_token: Option<&str>,
    component_id: &str,
    timeout_secs: u64,
) -> anyhow::Result<Vec<SmdEthernetInterface>> {
    let client =
        reqwest::Client::builder().timeout(std::time::Duration::from_secs(timeout_secs)).build()?;

    let url = format!(
        "{}/hsm/v2/Inventory/EthernetInterfaces?ComponentID={}",
        smd_url.trim_end_matches('/'),
        component_id
    );
    let mut req = client.get(&url);
    if let Some(token) = smd_token {
        req = req.header("Authorization", format!("Bearer {token}"));
    }

    let resp = req.send().await?;
    if !resp.status().is_success() {
        // Non-fatal: some nodes may not have ethernet info in SMD
        return Ok(Vec::new());
    }

    let interfaces: SmdEthernetInterfacesResponse =
        resp.json().await.unwrap_or(SmdEthernetInterfacesResponse(Vec::new()));
    Ok(interfaces.0)
}

/// Parse SMD components and ethernet interfaces into enrollment data.
pub fn parse_smd_enrollment_data(
    components: &[SmdComponent],
    interfaces: &[SmdEthernetInterface],
) -> Vec<(String, String)> {
    components
        .iter()
        .map(|c| {
            let mac = interfaces
                .iter()
                .find(|iface| iface.component_id == c.id)
                .map(|iface| iface.mac_address.clone())
                .unwrap_or_default();
            (c.id.clone(), mac)
        })
        .collect()
}

/// Discover nodes from OpenCHAMI SMD and batch-enroll them.
pub async fn import_from_smd(
    channel: &Channel,
    token: &str,
    smd_url: &str,
    smd_token: Option<&str>,
    group: Option<&str>,
    timeout_secs: u64,
) -> anyhow::Result<String> {
    // 1. Query SMD for node components
    let components = query_smd_components(smd_url, smd_token, group, timeout_secs).await?;

    if components.is_empty() {
        return Ok("No nodes found in SMD inventory.".to_string());
    }

    // 2. Query ethernet interfaces for all discovered components
    let mut all_interfaces = Vec::new();
    for component in &components {
        let ifaces = query_smd_ethernet(smd_url, smd_token, &component.id, timeout_secs).await?;
        all_interfaces.extend(ifaces);
    }

    // 3. Parse into enrollment data
    let enrollment_data = parse_smd_enrollment_data(&components, &all_interfaces);

    // 4. Enroll each node via gRPC
    let mut enrolled = 0u32;
    let mut already_enrolled = 0u32;
    let mut failed = 0u32;
    let mut errors = Vec::new();

    let mut client = EnrollmentServiceClient::new(channel.clone());

    for (node_id, mac) in &enrollment_data {
        let mut request = Request::new(RegisterNodeRequest {
            node_id: node_id.clone(),
            hardware_identity: Some(HardwareIdentity {
                mac_address: mac.clone(),
                bmc_serial: String::new(),
                extra: std::collections::HashMap::new(),
            }),
        });
        request.metadata_mut().insert("authorization", format!("Bearer {token}").parse().unwrap());

        match client.register_node(request).await {
            Ok(_) => enrolled += 1,
            Err(e) => {
                let msg = e.message().to_string();
                if msg.contains("already enrolled") || msg.contains("already registered") {
                    already_enrolled += 1;
                } else {
                    failed += 1;
                    errors.push(format!("  {node_id}: {msg}"));
                }
            }
        }
    }

    let mut output = format!(
        "Imported from SMD: {} enrolled, {} already enrolled, {} failed (out of {} discovered)",
        enrolled,
        already_enrolled,
        failed,
        enrollment_data.len()
    );
    for err in &errors {
        output.push('\n');
        output.push_str(err);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_smd_components_response() {
        let json = r#"{
            "Components": [
                {"ID": "x1000c0s0b0n0", "State": "Ready", "Type": "Node", "Role": "Compute"},
                {"ID": "x1000c0s0b0n1", "State": "Off", "Type": "Node", "Role": "Service"}
            ]
        }"#;
        let resp: SmdComponentsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.components.len(), 2);
        assert_eq!(resp.components[0].id, "x1000c0s0b0n0");
        assert_eq!(resp.components[0].role, "Compute");
        assert_eq!(resp.components[1].state, "Off");
    }

    #[test]
    fn parse_smd_ethernet_interfaces_response() {
        let json = r#"[
            {"ID": "x1000c0s0b0n0:eth0", "MACAddress": "aa:bb:cc:dd:ee:ff", "ComponentID": "x1000c0s0b0n0"},
            {"ID": "x1000c0s0b0n1:eth0", "MACAddress": "11:22:33:44:55:66", "ComponentID": "x1000c0s0b0n1"}
        ]"#;
        let resp: SmdEthernetInterfacesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.0.len(), 2);
        assert_eq!(resp.0[0].mac_address, "aa:bb:cc:dd:ee:ff");
        assert_eq!(resp.0[1].component_id, "x1000c0s0b0n1");
    }

    #[test]
    fn parse_smd_enrollment_data_matches_components_to_interfaces() {
        let components = vec![
            SmdComponent {
                id: "x1000c0s0b0n0".to_string(),
                state: "Ready".to_string(),
                component_type: "Node".to_string(),
                role: "Compute".to_string(),
            },
            SmdComponent {
                id: "x1000c0s0b0n1".to_string(),
                state: "Ready".to_string(),
                component_type: "Node".to_string(),
                role: "Compute".to_string(),
            },
        ];
        let interfaces = vec![SmdEthernetInterface {
            id: "x1000c0s0b0n0:eth0".to_string(),
            mac_address: "aa:bb:cc:dd:ee:ff".to_string(),
            component_id: "x1000c0s0b0n0".to_string(),
        }];

        let result = parse_smd_enrollment_data(&components, &interfaces);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], ("x1000c0s0b0n0".to_string(), "aa:bb:cc:dd:ee:ff".to_string()));
        // Second component has no interface, so MAC is empty
        assert_eq!(result[1], ("x1000c0s0b0n1".to_string(), String::new()));
    }

    #[test]
    fn parse_smd_enrollment_data_empty_inputs() {
        let result = parse_smd_enrollment_data(&[], &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn parse_smd_components_with_missing_optional_fields() {
        let json = r#"{
            "Components": [
                {"ID": "x1000c0s0b0n0"}
            ]
        }"#;
        let resp: SmdComponentsResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.components.len(), 1);
        assert_eq!(resp.components[0].id, "x1000c0s0b0n0");
        assert!(resp.components[0].state.is_empty());
        assert!(resp.components[0].role.is_empty());
    }
}
