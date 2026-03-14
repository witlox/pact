//! Connected MCP dispatch — wires tool calls to journal and agent gRPC.
//!
//! When the MCP server has connections, tool calls go through
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
use pact_common::proto::shell::{
    exec_output, shell_service_client::ShellServiceClient, ExecRequest, ListCommandsRequest,
};

use crate::protocol::{tool_result, ToolCallResult};

/// Holds optional connections to journal and agent.
pub struct Connections {
    pub journal: Option<Channel>,
    pub agent: Option<Channel>,
}

/// Dispatch a tool call using available gRPC connections.
///
/// Returns `None` only if the tool name is unknown.
pub async fn dispatch_connected(
    name: &str,
    arguments: &serde_json::Value,
    connections: &Connections,
) -> Option<ToolCallResult> {
    let no_journal = || tool_result("Error: not connected to journal".to_string(), true);
    let no_agent = || tool_result("Error: not connected to agent".to_string(), true);

    match name {
        // Journal-targeted tools
        "pact_status" => Some(match &connections.journal {
            Some(ch) => handle_status(arguments, ch).await,
            None => no_journal(),
        }),
        "pact_log" => Some(match &connections.journal {
            Some(ch) => handle_log(arguments, ch).await,
            None => no_journal(),
        }),
        "pact_commit" => Some(match &connections.journal {
            Some(ch) => handle_commit(arguments, ch).await,
            None => no_journal(),
        }),
        "pact_rollback" => Some(match &connections.journal {
            Some(ch) => handle_rollback(arguments, ch).await,
            None => no_journal(),
        }),
        "pact_diff" => Some(match &connections.journal {
            Some(ch) => handle_diff(arguments, ch).await,
            None => no_journal(),
        }),
        "pact_emergency" => Some(match &connections.journal {
            Some(ch) => handle_emergency(arguments, ch).await,
            None => no_journal(),
        }),
        "pact_apply" => Some(match &connections.journal {
            Some(ch) => handle_apply(arguments, ch).await,
            None => no_journal(),
        }),
        "pact_query_fleet" => Some(match &connections.journal {
            Some(ch) => handle_query_fleet(arguments, ch).await,
            None => no_journal(),
        }),
        // Agent-targeted tools
        "pact_exec" => Some(match &connections.agent {
            Some(ch) => handle_exec(arguments, ch).await,
            None => no_agent(),
        }),
        "pact_cap" => Some(match &connections.agent {
            Some(ch) => handle_cap(arguments, ch).await,
            None => no_agent(),
        }),
        "pact_service_status" => Some(match &connections.agent {
            Some(ch) => handle_service_status(arguments, ch).await,
            None => no_agent(),
        }),
        _ => None,
    }
}

/// Keep the old function for backwards compatibility with e2e tests.
pub async fn dispatch_tool_connected(
    name: &str,
    arguments: &serde_json::Value,
    channel: &Channel,
) -> Option<ToolCallResult> {
    let connections = Connections { journal: Some(channel.clone()), agent: None };
    dispatch_connected(name, arguments, &connections).await
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

// --- Agent-targeted tool handlers ---

async fn handle_exec(args: &serde_json::Value, channel: &Channel) -> ToolCallResult {
    let node = match args.get("node").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return tool_result("Error: node ID required".to_string(), true),
    };
    let command = match args.get("command").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return tool_result("Error: command required".to_string(), true),
    };

    // Split command into binary + args
    let parts: Vec<&str> = command.split_whitespace().collect();
    let (cmd, cmd_args) = match parts.split_first() {
        Some((first, rest)) => (*first, rest.iter().map(ToString::to_string).collect::<Vec<_>>()),
        None => return tool_result("Error: empty command".to_string(), true),
    };

    let mut client = ShellServiceClient::new(channel.clone());
    let mut request = tonic::Request::new(ExecRequest {
        command: cmd.to_string(),
        args: cmd_args,
    });
    // MCP server authenticates as pact-service-ai
    if let Ok(val) = "Bearer mcp-service-token".parse() {
        request.metadata_mut().insert("authorization", val);
    }

    match client.exec(request).await {
        Ok(resp) => {
            let mut stream = resp.into_inner();
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let mut exit_code = 0i32;

            while let Some(Ok(o)) = stream.next().await {
                match o.output {
                    Some(exec_output::Output::Stdout(data)) => stdout.extend_from_slice(&data),
                    Some(exec_output::Output::Stderr(data)) => stderr.extend_from_slice(&data),
                    Some(exec_output::Output::ExitCode(code)) => exit_code = code,
                    Some(exec_output::Output::Error(e)) => {
                        return tool_result(format!("Exec error: {e}"), true);
                    }
                    None => {}
                }
            }

            let mut output = format!("[{node}] {command}\n");
            if !stdout.is_empty() {
                output.push_str(&String::from_utf8_lossy(&stdout));
            }
            if !stderr.is_empty() {
                output.push_str(&format!("\nstderr: {}", String::from_utf8_lossy(&stderr)));
            }
            if exit_code != 0 {
                output.push_str(&format!("\n(exit code: {exit_code})"));
            }
            tool_result(output, exit_code != 0)
        }
        Err(e) => tool_result(format!("Exec failed: {e}"), true),
    }
}

async fn handle_cap(_args: &serde_json::Value, channel: &Channel) -> ToolCallResult {
    let mut client = ShellServiceClient::new(channel.clone());
    match client
        .list_commands(tonic::Request::new(ListCommandsRequest {}))
        .await
    {
        Ok(resp) => {
            let commands = resp.into_inner().commands;
            if commands.is_empty() {
                return tool_result("No commands available.".to_string(), false);
            }
            let mut output = format!("{} whitelisted commands:\n", commands.len());
            for cmd in &commands {
                let state = if cmd.state_changing { "[state-changing]" } else { "" };
                output.push_str(&format!("  {} {state} {}\n", cmd.command, cmd.description));
            }
            tool_result(output, false)
        }
        Err(e) => tool_result(format!("Cap query failed: {e}"), true),
    }
}

async fn handle_service_status(args: &serde_json::Value, channel: &Channel) -> ToolCallResult {
    let service = args.get("service").and_then(|v| v.as_str()).unwrap_or("--all");
    let mut client = ShellServiceClient::new(channel.clone());

    let mut request = tonic::Request::new(ExecRequest {
        command: "systemctl".to_string(),
        args: vec!["status".to_string(), service.to_string()],
    });
    if let Ok(val) = "Bearer mcp-service-token".parse() {
        request.metadata_mut().insert("authorization", val);
    }

    match client.exec(request).await {
        Ok(resp) => {
            let mut stream = resp.into_inner();
            let mut output = String::new();
            while let Some(Ok(o)) = stream.next().await {
                match o.output {
                    Some(exec_output::Output::Stdout(data)) => {
                        output.push_str(&String::from_utf8_lossy(&data));
                    }
                    Some(exec_output::Output::Error(e)) => {
                        return tool_result(format!("Service status error: {e}"), true);
                    }
                    _ => {}
                }
            }
            tool_result(output, false)
        }
        Err(e) => tool_result(format!("Service status failed: {e}"), true),
    }
}

async fn handle_apply(args: &serde_json::Value, channel: &Channel) -> ToolCallResult {
    let scope = match args.get("scope").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return tool_result("Error: scope required".to_string(), true),
    };
    let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("mcp-apply");
    let config = match args.get("config") {
        Some(c) => c,
        None => return tool_result("Error: config required".to_string(), true),
    };

    // Serialize config as the policy_ref for the entry
    let config_str = serde_json::to_string(config).unwrap_or_default();

    let mut client = ConfigServiceClient::new(channel.clone());
    let entry = ProtoConfigEntry {
        sequence: 0,
        timestamp: None,
        entry_type: 1, // Commit
        scope: Some(ProtoScopeMsg {
            scope: Some(ProtoScope::VclusterId(scope.to_string())),
        }),
        author: Some(ProtoIdentity {
            principal: "mcp-agent".to_string(),
            principal_type: "Agent".to_string(),
            role: "pact-service-ai".to_string(),
        }),
        parent: None,
        state_delta: None,
        policy_ref: format!("{message}: {config_str}"),
        ttl: None,
        emergency_reason: None,
    };

    match client
        .append_entry(tonic::Request::new(AppendEntryRequest { entry: Some(entry) }))
        .await
    {
        Ok(resp) => {
            let seq = resp.into_inner().sequence;
            tool_result(format!("Applied to {scope} (seq:{seq}): {message}"), false)
        }
        Err(e) => tool_result(format!("Apply failed: {e}"), true),
    }
}

async fn handle_query_fleet(args: &serde_json::Value, channel: &Channel) -> ToolCallResult {
    // Query fleet by listing all entries and filtering by vCluster
    let vcluster = args.get("vcluster").and_then(|v| v.as_str()).unwrap_or("all");
    let scope = if vcluster == "all" {
        None
    } else {
        Some(ProtoScopeMsg {
            scope: Some(ProtoScope::VclusterId(vcluster.to_string())),
        })
    };

    let mut client = ConfigServiceClient::new(channel.clone());
    match client
        .list_entries(tonic::Request::new(ListEntriesRequest {
            scope,
            from_sequence: None,
            to_sequence: None,
            limit: Some(100),
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
                tool_result(format!("No entries for vCluster: {vcluster}"), false)
            } else {
                tool_result(
                    format!("Fleet query ({vcluster}): {} entries\n{}", entries.len(), entries.join("\n")),
                    false,
                )
            }
        }
        Err(e) => tool_result(format!("Fleet query failed: {e}"), true),
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
