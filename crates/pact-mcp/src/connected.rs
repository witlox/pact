//! Connected MCP dispatch — wires tool calls to journal gRPC.
//!
//! When the MCP server has a journal connection, tool calls go through
//! real gRPC instead of returning stubs.

use tokio_stream::StreamExt;
use tonic::transport::Channel;

use pact_common::proto::config::{
    scope::Scope as ProtoScope, ConfigEntry as ProtoConfigEntry, Identity as ProtoIdentity,
    Scope as ProtoScopeMsg,
};
use pact_common::proto::journal::config_service_client::ConfigServiceClient;
use pact_common::proto::journal::{
    AppendEntryRequest, GetNodeStateRequest, ListEntriesRequest,
};

use crate::protocol::{tool_result, ToolCallResult};

/// Dispatch a tool call using a real journal gRPC connection.
///
/// Returns `None` if the tool is not journal-connected (agent tools),
/// in which case the caller should fall back to the stub dispatch.
pub async fn dispatch_tool_connected(
    name: &str,
    arguments: &serde_json::Value,
    channel: &Channel,
) -> Option<ToolCallResult> {
    match name {
        "pact_status" => Some(handle_status(arguments, channel).await),
        "pact_log" => Some(handle_log(arguments, channel).await),
        "pact_commit" => Some(handle_commit(arguments, channel).await),
        "pact_rollback" => Some(handle_rollback(arguments, channel).await),
        "pact_diff" => Some(handle_diff(arguments, channel).await),
        "pact_emergency" => Some(handle_emergency(arguments, channel).await),
        // Agent-targeted tools fall through to stub
        _ => None,
    }
}

async fn handle_status(args: &serde_json::Value, channel: &Channel) -> ToolCallResult {
    let node = args.get("node").and_then(|v| v.as_str()).unwrap_or("local");
    let mut client = ConfigServiceClient::new(channel.clone());

    match client
        .get_node_state(tonic::Request::new(GetNodeStateRequest {
            node_id: node.to_string(),
        }))
        .await
    {
        Ok(resp) => {
            let ns = resp.into_inner();
            tool_result(format!("Node: {}  State: {}", ns.node_id, ns.config_state), false)
        }
        Err(e) => tool_result(format!("Error querying status: {e}"), true),
    }
}

async fn handle_log(args: &serde_json::Value, channel: &Channel) -> ToolCallResult {
    let n = args.get("n").and_then(serde_json::Value::as_u64).unwrap_or(20) as u32;
    let scope = args.get("scope").and_then(|v| v.as_str());
    let mut client = ConfigServiceClient::new(channel.clone());

    let scope_proto = scope.map(|s| {
        if let Some(node) = s.strip_prefix("node:") {
            ProtoScopeMsg { scope: Some(ProtoScope::NodeId(node.to_string())) }
        } else if let Some(vc) = s.strip_prefix("vc:") {
            ProtoScopeMsg { scope: Some(ProtoScope::VclusterId(vc.to_string())) }
        } else {
            ProtoScopeMsg { scope: Some(ProtoScope::Global(true)) }
        }
    });

    match client
        .list_entries(tonic::Request::new(ListEntriesRequest {
            scope: scope_proto,
            from_sequence: None,
            to_sequence: None,
            limit: Some(n),
        }))
        .await
    {
        Ok(resp) => {
            let mut stream = resp.into_inner();
            let mut entries = Vec::new();
            while let Some(Ok(entry)) = stream.next().await {
                entries.push(format_entry(&entry));
            }
            if entries.is_empty() {
                tool_result("No entries found.".to_string(), false)
            } else {
                tool_result(entries.join("\n"), false)
            }
        }
        Err(e) => tool_result(format!("Error querying log: {e}"), true),
    }
}

async fn handle_commit(args: &serde_json::Value, channel: &Channel) -> ToolCallResult {
    let message = match args.get("message").and_then(|v| v.as_str()) {
        Some(m) => m,
        None => return tool_result("Error: commit message required", true),
    };
    let vcluster = args.get("vcluster").and_then(|v| v.as_str()).unwrap_or("default");

    let mut client = ConfigServiceClient::new(channel.clone());
    let entry = ProtoConfigEntry {
        sequence: 0,
        timestamp: None,
        entry_type: 1, // Commit
        scope: Some(ProtoScopeMsg {
            scope: Some(ProtoScope::VclusterId(vcluster.to_string())),
        }),
        author: Some(ProtoIdentity {
            principal: "mcp-agent".to_string(),
            principal_type: "Agent".to_string(),
            role: "pact-service-ai".to_string(),
        }),
        parent: None,
        state_delta: None,
        policy_ref: message.to_string(),
        ttl: None,
        emergency_reason: None,
    };

    match client
        .append_entry(tonic::Request::new(AppendEntryRequest { entry: Some(entry) }))
        .await
    {
        Ok(resp) => {
            let seq = resp.into_inner().sequence;
            tool_result(format!("Committed (seq:{seq}) on vCluster: {vcluster}"), false)
        }
        Err(e) => tool_result(format!("Commit failed: {e}"), true),
    }
}

async fn handle_rollback(args: &serde_json::Value, channel: &Channel) -> ToolCallResult {
    let seq = match args.get("sequence").and_then(serde_json::Value::as_u64) {
        Some(s) => s,
        None => return tool_result("Error: sequence number required", true),
    };
    let vcluster = args.get("vcluster").and_then(|v| v.as_str()).unwrap_or("default");

    let mut client = ConfigServiceClient::new(channel.clone());
    let entry = ProtoConfigEntry {
        sequence: 0,
        timestamp: None,
        entry_type: 2, // Rollback
        scope: Some(ProtoScopeMsg {
            scope: Some(ProtoScope::VclusterId(vcluster.to_string())),
        }),
        author: Some(ProtoIdentity {
            principal: "mcp-agent".to_string(),
            principal_type: "Agent".to_string(),
            role: "pact-service-ai".to_string(),
        }),
        parent: Some(seq),
        state_delta: None,
        policy_ref: String::new(),
        ttl: None,
        emergency_reason: None,
    };

    match client
        .append_entry(tonic::Request::new(AppendEntryRequest { entry: Some(entry) }))
        .await
    {
        Ok(resp) => {
            let new_seq = resp.into_inner().sequence;
            tool_result(
                format!("Rolled back to seq:{seq} (new seq:{new_seq})"),
                false,
            )
        }
        Err(e) => tool_result(format!("Rollback failed: {e}"), true),
    }
}

async fn handle_diff(args: &serde_json::Value, channel: &Channel) -> ToolCallResult {
    // Diff uses log with node scope filter
    let node = args.get("node").and_then(|v| v.as_str()).unwrap_or("current");
    let mut modified_args = args.clone();
    modified_args["scope"] = serde_json::json!(format!("node:{node}"));
    modified_args["n"] = serde_json::json!(50);
    handle_log(&modified_args, channel).await
}

async fn handle_emergency(args: &serde_json::Value, channel: &Channel) -> ToolCallResult {
    let action = match args.get("action").and_then(|v| v.as_str()) {
        Some(a) => a,
        None => return tool_result("Error: action required (start/end)", true),
    };

    // P8: AI agents cannot enter emergency mode
    if action == "start" {
        return tool_result(
            "Error: AI agents are restricted from entering emergency mode (P8). \
             Request a human admin to enter emergency mode.",
            true,
        );
    }

    // Only "end" reaches here
    let vcluster = args.get("vcluster").and_then(|v| v.as_str()).unwrap_or("default");
    let mut client = ConfigServiceClient::new(channel.clone());
    let entry = ProtoConfigEntry {
        sequence: 0,
        timestamp: None,
        entry_type: 9, // EmergencyEnd
        scope: Some(ProtoScopeMsg {
            scope: Some(ProtoScope::VclusterId(vcluster.to_string())),
        }),
        author: Some(ProtoIdentity {
            principal: "mcp-agent".to_string(),
            principal_type: "Agent".to_string(),
            role: "pact-service-ai".to_string(),
        }),
        parent: None,
        state_delta: None,
        policy_ref: String::new(),
        ttl: None,
        emergency_reason: None,
    };

    match client
        .append_entry(tonic::Request::new(AppendEntryRequest { entry: Some(entry) }))
        .await
    {
        Ok(resp) => {
            let seq = resp.into_inner().sequence;
            tool_result(format!("Emergency mode ENDED (seq:{seq})"), false)
        }
        Err(e) => tool_result(format!("Emergency end failed: {e}"), true),
    }
}

fn format_entry(entry: &ProtoConfigEntry) -> String {
    let entry_type_name = match entry.entry_type {
        1 => "COMMIT",
        2 => "ROLLBACK",
        3 => "AUTO_CONVERGE",
        4 => "DRIFT_DETECTED",
        5 => "CAPABILITY_CHANGE",
        6 => "POLICY_UPDATE",
        7 => "BOOT_CONFIG",
        8 => "EMERGENCY_ON",
        9 => "EMERGENCY_OFF",
        10 => "EXEC_LOG",
        11 => "SHELL_SESSION",
        12 => "SERVICE_LIFECYCLE",
        13 => "PENDING_APPROVAL",
        _ => "UNKNOWN",
    };

    let scope = entry.scope.as_ref().map_or_else(
        || "global".to_string(),
        |s| match &s.scope {
            Some(ProtoScope::NodeId(n)) => format!("node:{n}"),
            Some(ProtoScope::VclusterId(v)) => format!("vc:{v}"),
            _ => "global".to_string(),
        },
    );

    let author = entry
        .author
        .as_ref()
        .map_or_else(|| "unknown".to_string(), |a| a.principal.clone());

    format!("#{} {} {} by {}", entry.sequence, entry_type_name, scope, author)
}
