//! CLI commands for node enrollment, assignment, and management.
//!
//! Maps to EnrollmentService gRPC RPCs.

use tonic::transport::Channel;
use tonic::Request;

use pact_common::proto::enrollment::enrollment_service_client::EnrollmentServiceClient;
use pact_common::proto::enrollment::{
    AssignNodeRequest, DecommissionNodeRequest, HardwareIdentity, InspectNodeRequest,
    ListNodesRequest, MoveNodeRequest, RegisterNodeRequest, UnassignNodeRequest,
};

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
