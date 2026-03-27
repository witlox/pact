//! Connected MCP dispatch — wires tool calls to journal and agent gRPC.
//!
//! When the MCP server has connections, tool calls go through
//! real gRPC instead of returning stubs.

use tokio_stream::StreamExt;
use tonic::transport::Channel;

use pact_common::config::DelegationConfig;
use pact_common::proto::config::{
    scope::Scope as ProtoScope, ConfigEntry as ProtoConfigEntry, Identity as ProtoIdentity,
    Scope as ProtoScopeMsg,
};
use pact_common::proto::journal::{AppendEntryRequest, GetNodeStateRequest, ListEntriesRequest};
use pact_common::proto::shell::{
    exec_output, shell_service_client::ShellServiceClient, ExecRequest, ListCommandsRequest,
};

use pact_cli::commands::execute::AuthInterceptor;

use crate::protocol::{tool_result, ToolCallResult};

/// Holds optional connections to journal and agent, plus lattice delegation config.
pub struct Connections {
    pub journal: Option<Channel>,
    pub agent: Option<Channel>,
    pub delegation: DelegationConfig,
    /// Bearer token for MCP→journal+agent authentication (from PACT_MCP_TOKEN env var).
    pub mcp_token: Option<String>,
}

impl Connections {
    /// Create an authenticated journal config client.
    fn journal_config_client(
        &self,
    ) -> Option<
        pact_common::proto::journal::config_service_client::ConfigServiceClient<
            tonic::service::interceptor::InterceptedService<Channel, AuthInterceptor>,
        >,
    > {
        let channel = self.journal.as_ref()?;
        let token = self.mcp_token.clone().unwrap_or_default();
        Some(
            pact_common::proto::journal::config_service_client::ConfigServiceClient::with_interceptor(
                channel.clone(),
                AuthInterceptor::new(token),
            ),
        )
    }

    /// Create an authenticated journal policy client.
    #[allow(dead_code)] // Will be used when policy MCP tools are added
    fn journal_policy_client(
        &self,
    ) -> Option<
        pact_common::proto::policy::policy_service_client::PolicyServiceClient<
            tonic::service::interceptor::InterceptedService<Channel, AuthInterceptor>,
        >,
    > {
        let channel = self.journal.as_ref()?;
        let token = self.mcp_token.clone().unwrap_or_default();
        Some(
            pact_common::proto::policy::policy_service_client::PolicyServiceClient::with_interceptor(
                channel.clone(),
                AuthInterceptor::new(token),
            ),
        )
    }

    /// Create an authenticated agent shell client.
    fn agent_shell_client(
        &self,
    ) -> Option<
        ShellServiceClient<
            tonic::service::interceptor::InterceptedService<Channel, AuthInterceptor>,
        >,
    > {
        let channel = self.agent.as_ref()?;
        let token = self.mcp_token.clone().unwrap_or_default();
        Some(ShellServiceClient::with_interceptor(channel.clone(), AuthInterceptor::new(token)))
    }
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
            Some(_ch) => handle_status(arguments, connections).await,
            None => no_journal(),
        }),
        "pact_log" => Some(match &connections.journal {
            Some(_ch) => handle_log(arguments, connections).await,
            None => no_journal(),
        }),
        "pact_commit" => Some(match &connections.journal {
            Some(_ch) => handle_commit(arguments, connections).await,
            None => no_journal(),
        }),
        "pact_rollback" => Some(match &connections.journal {
            Some(_ch) => handle_rollback(arguments, connections).await,
            None => no_journal(),
        }),
        "pact_diff" => Some(match &connections.journal {
            Some(_ch) => handle_diff(arguments, connections).await,
            None => no_journal(),
        }),
        "pact_emergency" => Some(match &connections.journal {
            Some(_ch) => handle_emergency(arguments, connections).await,
            None => no_journal(),
        }),
        "pact_apply" => Some(match &connections.journal {
            Some(_ch) => handle_apply(arguments, connections).await,
            None => no_journal(),
        }),
        "pact_query_fleet" => Some(match &connections.journal {
            Some(_ch) => handle_query_fleet(arguments, connections).await,
            None => no_journal(),
        }),
        // Agent-targeted tools
        "pact_exec" => Some(match &connections.agent {
            Some(_ch) => handle_exec(arguments, connections).await,
            None => no_agent(),
        }),
        "pact_cap" => Some(match &connections.agent {
            Some(_ch) => handle_cap(arguments, connections).await,
            None => no_agent(),
        }),
        "pact_service_status" => Some(match &connections.agent {
            Some(_ch) => handle_service_status(arguments, connections).await,
            None => no_agent(),
        }),
        // Supercharged commands — delegated to lattice via DelegationConfig
        "pact_jobs_list" => Some(handle_jobs_list(arguments, &connections.delegation).await),
        "pact_queue_status" => Some(handle_queue_status(arguments, &connections.delegation).await),
        "pact_cluster_health" => Some(match &connections.journal {
            Some(_ch) => handle_cluster_health(arguments, connections).await,
            None => no_journal(),
        }),
        "pact_system_health" => Some(match &connections.journal {
            Some(_ch) => handle_system_health(arguments, connections).await,
            None => no_journal(),
        }),
        "pact_accounting" => Some(handle_accounting(arguments, &connections.delegation).await),
        "pact_services_list" => {
            Some(handle_services_list(arguments, &connections.delegation).await)
        }
        "pact_services_lookup" => {
            Some(handle_services_lookup(arguments, &connections.delegation).await)
        }
        // New lattice delegation tools
        "pact_undrain" => Some(match &connections.journal {
            Some(_ch) => handle_undrain(arguments, connections).await,
            None => no_journal(),
        }),
        "pact_dag_list" => Some(handle_dag_list(arguments, &connections.delegation).await),
        "pact_dag_inspect" => Some(handle_dag_inspect(arguments, &connections.delegation).await),
        "pact_budget" => Some(handle_budget(arguments, &connections.delegation).await),
        "pact_backup_create" => Some(match &connections.journal {
            Some(_ch) => handle_backup_create(arguments, connections).await,
            None => no_journal(),
        }),
        "pact_backup_verify" => {
            Some(handle_backup_verify(arguments, &connections.delegation).await)
        }
        "pact_nodes_list" => Some(handle_nodes_list(arguments, &connections.delegation).await),
        "pact_node_inspect" => Some(handle_node_inspect(arguments, &connections.delegation).await),
        _ => None,
    }
}

/// Keep the old function for backwards compatibility with e2e tests.
pub async fn dispatch_tool_connected(
    name: &str,
    arguments: &serde_json::Value,
    channel: &Channel,
) -> Option<ToolCallResult> {
    let connections = Connections {
        journal: Some(channel.clone()),
        agent: None,
        delegation: DelegationConfig::default(),
        mcp_token: None,
    };
    dispatch_connected(name, arguments, &connections).await
}

async fn handle_status(args: &serde_json::Value, connections: &Connections) -> ToolCallResult {
    let node = args.get("node").and_then(|v| v.as_str()).unwrap_or("local");
    let mut client = connections.journal_config_client().unwrap();

    match client
        .get_node_state(tonic::Request::new(GetNodeStateRequest { node_id: node.to_string() }))
        .await
    {
        Ok(resp) => {
            let ns = resp.into_inner();
            tool_result(format!("Node: {}  State: {}", ns.node_id, ns.config_state), false)
        }
        Err(e) => tool_result(format!("Error querying status: {e}"), true),
    }
}

async fn handle_log(args: &serde_json::Value, connections: &Connections) -> ToolCallResult {
    let n = args.get("n").and_then(serde_json::Value::as_u64).unwrap_or(20) as u32;
    let scope = args.get("scope").and_then(|v| v.as_str());
    let mut client = connections.journal_config_client().unwrap();

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

async fn handle_commit(args: &serde_json::Value, connections: &Connections) -> ToolCallResult {
    let message = match args.get("message").and_then(|v| v.as_str()) {
        Some(m) => m,
        None => return tool_result("Error: commit message required", true),
    };
    let vcluster = args.get("vcluster").and_then(|v| v.as_str()).unwrap_or("default");

    let mut client = connections.journal_config_client().unwrap();
    let entry = ProtoConfigEntry {
        sequence: 0,
        timestamp: None,
        entry_type: 1, // Commit
        scope: Some(ProtoScopeMsg { scope: Some(ProtoScope::VclusterId(vcluster.to_string())) }),
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

    match client.append_entry(tonic::Request::new(AppendEntryRequest { entry: Some(entry) })).await
    {
        Ok(resp) => {
            let seq = resp.into_inner().sequence;
            tool_result(format!("Committed (seq:{seq}) on vCluster: {vcluster}"), false)
        }
        Err(e) => tool_result(format!("Commit failed: {e}"), true),
    }
}

async fn handle_rollback(args: &serde_json::Value, connections: &Connections) -> ToolCallResult {
    let seq = match args.get("sequence").and_then(serde_json::Value::as_u64) {
        Some(s) => s,
        None => return tool_result("Error: sequence number required", true),
    };
    let vcluster = args.get("vcluster").and_then(|v| v.as_str()).unwrap_or("default");

    let mut client = connections.journal_config_client().unwrap();
    let entry = ProtoConfigEntry {
        sequence: 0,
        timestamp: None,
        entry_type: 2, // Rollback
        scope: Some(ProtoScopeMsg { scope: Some(ProtoScope::VclusterId(vcluster.to_string())) }),
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

    match client.append_entry(tonic::Request::new(AppendEntryRequest { entry: Some(entry) })).await
    {
        Ok(resp) => {
            let new_seq = resp.into_inner().sequence;
            tool_result(format!("Rolled back to seq:{seq} (new seq:{new_seq})"), false)
        }
        Err(e) => tool_result(format!("Rollback failed: {e}"), true),
    }
}

async fn handle_diff(args: &serde_json::Value, connections: &Connections) -> ToolCallResult {
    // Diff uses log with node scope filter
    let node = args.get("node").and_then(|v| v.as_str()).unwrap_or("current");
    let mut modified_args = args.clone();
    modified_args["scope"] = serde_json::json!(format!("node:{node}"));
    modified_args["n"] = serde_json::json!(50);
    handle_log(&modified_args, connections).await
}

async fn handle_emergency(args: &serde_json::Value, connections: &Connections) -> ToolCallResult {
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
    let mut client = connections.journal_config_client().unwrap();
    let entry = ProtoConfigEntry {
        sequence: 0,
        timestamp: None,
        entry_type: 9, // EmergencyEnd
        scope: Some(ProtoScopeMsg { scope: Some(ProtoScope::VclusterId(vcluster.to_string())) }),
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

    match client.append_entry(tonic::Request::new(AppendEntryRequest { entry: Some(entry) })).await
    {
        Ok(resp) => {
            let seq = resp.into_inner().sequence;
            tool_result(format!("Emergency mode ENDED (seq:{seq})"), false)
        }
        Err(e) => tool_result(format!("Emergency end failed: {e}"), true),
    }
}

// --- Agent-targeted tool handlers ---

async fn handle_exec(args: &serde_json::Value, connections: &Connections) -> ToolCallResult {
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

    let mut client = connections.agent_shell_client().unwrap();
    let request = tonic::Request::new(ExecRequest { command: cmd.to_string(), args: cmd_args });
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

async fn handle_cap(_args: &serde_json::Value, connections: &Connections) -> ToolCallResult {
    let mut client = connections.agent_shell_client().unwrap();
    match client.list_commands(tonic::Request::new(ListCommandsRequest {})).await {
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

async fn handle_service_status(
    args: &serde_json::Value,
    connections: &Connections,
) -> ToolCallResult {
    let service = args.get("service").and_then(|v| v.as_str()).unwrap_or("--all");
    let mut client = connections.agent_shell_client().unwrap();

    let request = tonic::Request::new(ExecRequest {
        command: "systemctl".to_string(),
        args: vec!["status".to_string(), service.to_string()],
    });
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

async fn handle_apply(args: &serde_json::Value, connections: &Connections) -> ToolCallResult {
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

    let mut client = connections.journal_config_client().unwrap();
    let entry = ProtoConfigEntry {
        sequence: 0,
        timestamp: None,
        entry_type: 1, // Commit
        scope: Some(ProtoScopeMsg { scope: Some(ProtoScope::VclusterId(scope.to_string())) }),
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

    match client.append_entry(tonic::Request::new(AppendEntryRequest { entry: Some(entry) })).await
    {
        Ok(resp) => {
            let seq = resp.into_inner().sequence;
            tool_result(format!("Applied to {scope} (seq:{seq}): {message}"), false)
        }
        Err(e) => tool_result(format!("Apply failed: {e}"), true),
    }
}

async fn handle_query_fleet(args: &serde_json::Value, connections: &Connections) -> ToolCallResult {
    // Query fleet by listing all entries and filtering by vCluster
    let vcluster = args.get("vcluster").and_then(|v| v.as_str()).unwrap_or("all");
    let scope = if vcluster == "all" {
        None
    } else {
        Some(ProtoScopeMsg { scope: Some(ProtoScope::VclusterId(vcluster.to_string())) })
    };

    let mut client = connections.journal_config_client().unwrap();
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
                    format!(
                        "Fleet query ({vcluster}): {} entries\n{}",
                        entries.len(),
                        entries.join("\n")
                    ),
                    false,
                )
            }
        }
        Err(e) => tool_result(format!("Fleet query failed: {e}"), true),
    }
}

// --- Supercharged command handlers (lattice delegation) ---

async fn handle_jobs_list(args: &serde_json::Value, config: &DelegationConfig) -> ToolCallResult {
    let node = args.get("node").and_then(|v| v.as_str());
    let vcluster = args.get("vcluster").and_then(|v| v.as_str());

    match pact_cli::commands::lattice::list_jobs(config, node, vcluster).await {
        Ok(output) => tool_result(output, false),
        Err(e) => tool_result(format!("Jobs list failed: {e}"), true),
    }
}

async fn handle_queue_status(
    args: &serde_json::Value,
    config: &DelegationConfig,
) -> ToolCallResult {
    let vcluster = args.get("vcluster").and_then(|v| v.as_str());

    match pact_cli::commands::lattice::queue_status(config, vcluster).await {
        Ok(output) => tool_result(output, false),
        Err(e) => tool_result(format!("Queue status failed: {e}"), true),
    }
}

async fn handle_cluster_health(
    _args: &serde_json::Value,
    connections: &Connections,
) -> ToolCallResult {
    let mut client = connections.journal_config_client().unwrap();
    match pact_cli::commands::lattice::cluster_status(&mut client, &connections.delegation).await {
        Ok(output) => tool_result(output, false),
        Err(e) => tool_result(format!("Cluster health failed: {e}"), true),
    }
}

async fn handle_system_health(
    _args: &serde_json::Value,
    connections: &Connections,
) -> ToolCallResult {
    let mut client = connections.journal_config_client().unwrap();
    match pact_cli::commands::lattice::health_check(&mut client, &connections.delegation).await {
        Ok(output) => tool_result(output, false),
        Err(e) => tool_result(format!("System health check failed: {e}"), true),
    }
}

async fn handle_accounting(args: &serde_json::Value, config: &DelegationConfig) -> ToolCallResult {
    let vcluster = args.get("vcluster").and_then(|v| v.as_str());

    match pact_cli::commands::lattice::accounting(config, vcluster).await {
        Ok(output) => tool_result(output, false),
        Err(e) => tool_result(format!("Accounting query failed: {e}"), true),
    }
}

async fn handle_services_list(
    _args: &serde_json::Value,
    config: &DelegationConfig,
) -> ToolCallResult {
    match pact_cli::commands::lattice::list_services(config).await {
        Ok(output) => tool_result(output, false),
        Err(e) => tool_result(format!("Service list failed: {e}"), true),
    }
}

async fn handle_services_lookup(
    args: &serde_json::Value,
    config: &DelegationConfig,
) -> ToolCallResult {
    let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");

    match pact_cli::commands::lattice::lookup_service(config, name).await {
        Ok(output) => tool_result(output, false),
        Err(e) => tool_result(format!("Service lookup failed: {e}"), true),
    }
}

// --- New lattice delegation handlers ---

async fn handle_undrain(args: &serde_json::Value, connections: &Connections) -> ToolCallResult {
    let node = match args.get("node").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return tool_result("Error: node ID required".to_string(), true),
    };

    let mut client = connections.journal_config_client().unwrap();
    let result = pact_cli::commands::delegate::undrain_node(
        &mut client,
        node,
        "mcp-agent",
        "pact-service-ai",
        &connections.delegation,
    )
    .await;

    tool_result(pact_cli::commands::delegate::format_delegation_result(&result), !result.success)
}

async fn handle_dag_list(args: &serde_json::Value, config: &DelegationConfig) -> ToolCallResult {
    let tenant = args.get("tenant").and_then(|v| v.as_str());
    let state = args.get("state").and_then(|v| v.as_str());
    let limit = args.get("limit").and_then(serde_json::Value::as_u64).unwrap_or(50) as u32;

    match pact_cli::commands::lattice::list_dags(config, tenant, state, limit).await {
        Ok(output) => tool_result(output, false),
        Err(e) => tool_result(format!("DAG list failed: {e}"), true),
    }
}

async fn handle_dag_inspect(args: &serde_json::Value, config: &DelegationConfig) -> ToolCallResult {
    let dag_id = match args.get("dag_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return tool_result("Error: dag_id required".to_string(), true),
    };

    match pact_cli::commands::lattice::inspect_dag(config, dag_id).await {
        Ok(output) => tool_result(output, false),
        Err(e) => tool_result(format!("DAG inspect failed: {e}"), true),
    }
}

async fn handle_budget(args: &serde_json::Value, config: &DelegationConfig) -> ToolCallResult {
    let days = args.get("days").and_then(serde_json::Value::as_u64).unwrap_or(90) as u32;

    if let Some(tenant) = args.get("tenant").and_then(|v| v.as_str()) {
        match pact_cli::commands::lattice::tenant_budget(config, tenant, days).await {
            Ok(output) => tool_result(output, false),
            Err(e) => tool_result(format!("Tenant budget failed: {e}"), true),
        }
    } else if let Some(user) = args.get("user").and_then(|v| v.as_str()) {
        match pact_cli::commands::lattice::user_budget(config, user, days).await {
            Ok(output) => tool_result(output, false),
            Err(e) => tool_result(format!("User budget failed: {e}"), true),
        }
    } else {
        tool_result("Error: specify tenant or user".to_string(), true)
    }
}

async fn handle_backup_create(
    args: &serde_json::Value,
    connections: &Connections,
) -> ToolCallResult {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return tool_result("Error: path required".to_string(), true),
    };

    let mut client = connections.journal_config_client().unwrap();
    match pact_cli::commands::lattice::backup_create(
        &mut client,
        &connections.delegation,
        path,
        "mcp-agent",
        "pact-service-ai",
    )
    .await
    {
        Ok(output) => tool_result(output, false),
        Err(e) => tool_result(format!("Backup create failed: {e}"), true),
    }
}

async fn handle_backup_verify(
    args: &serde_json::Value,
    config: &DelegationConfig,
) -> ToolCallResult {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return tool_result("Error: path required".to_string(), true),
    };

    match pact_cli::commands::lattice::backup_verify(config, path).await {
        Ok(output) => tool_result(output, false),
        Err(e) => tool_result(format!("Backup verify failed: {e}"), true),
    }
}

async fn handle_nodes_list(args: &serde_json::Value, config: &DelegationConfig) -> ToolCallResult {
    let state = args.get("state").and_then(|v| v.as_str());
    let vcluster = args.get("vcluster").and_then(|v| v.as_str());
    let limit = args.get("limit").and_then(serde_json::Value::as_u64).unwrap_or(100) as u32;

    match pact_cli::commands::lattice::list_lattice_nodes(config, state, vcluster, limit).await {
        Ok(output) => tool_result(output, false),
        Err(e) => tool_result(format!("Nodes list failed: {e}"), true),
    }
}

async fn handle_node_inspect(
    args: &serde_json::Value,
    config: &DelegationConfig,
) -> ToolCallResult {
    let node_id = match args.get("node_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return tool_result("Error: node_id required".to_string(), true),
    };

    match pact_cli::commands::lattice::inspect_lattice_node(config, node_id).await {
        Ok(output) => tool_result(output, false),
        Err(e) => tool_result(format!("Node inspect failed: {e}"), true),
    }
}

/// Format a config entry for display. Public for testing.
pub fn format_entry(entry: &ProtoConfigEntry) -> String {
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

    let author =
        entry.author.as_ref().map_or_else(|| "unknown".to_string(), |a| a.principal.clone());

    format!("#{} {} {} by {}", entry.sequence, entry_type_name, scope, author)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- format_entry tests ---

    fn make_entry(
        sequence: u64,
        entry_type: i32,
        scope: Option<ProtoScopeMsg>,
        author: Option<ProtoIdentity>,
    ) -> ProtoConfigEntry {
        ProtoConfigEntry {
            sequence,
            timestamp: None,
            entry_type,
            scope,
            author,
            parent: None,
            state_delta: None,
            policy_ref: String::new(),
            ttl: None,
            emergency_reason: None,
        }
    }

    #[test]
    fn format_entry_commit_with_vcluster() {
        let entry = make_entry(
            42,
            1,
            Some(ProtoScopeMsg { scope: Some(ProtoScope::VclusterId("ml-train".into())) }),
            Some(ProtoIdentity {
                principal: "alice@cscs.ch".into(),
                principal_type: "Human".into(),
                role: "pact-ops-ml".into(),
            }),
        );
        let formatted = format_entry(&entry);
        assert_eq!(formatted, "#42 COMMIT vc:ml-train by alice@cscs.ch");
    }

    #[test]
    fn format_entry_rollback_with_node_scope() {
        let entry = make_entry(
            7,
            2,
            Some(ProtoScopeMsg { scope: Some(ProtoScope::NodeId("node-099".into())) }),
            Some(ProtoIdentity {
                principal: "bob".into(),
                principal_type: "Human".into(),
                role: "admin".into(),
            }),
        );
        let formatted = format_entry(&entry);
        assert_eq!(formatted, "#7 ROLLBACK node:node-099 by bob");
    }

    #[test]
    fn format_entry_no_scope_no_author() {
        let entry = make_entry(1, 4, None, None);
        let formatted = format_entry(&entry);
        assert_eq!(formatted, "#1 DRIFT_DETECTED global by unknown");
    }

    #[test]
    fn format_entry_global_scope() {
        let entry = make_entry(
            5,
            6,
            Some(ProtoScopeMsg { scope: Some(ProtoScope::Global(true)) }),
            Some(ProtoIdentity {
                principal: "system".into(),
                principal_type: "Service".into(),
                role: "pact-platform-admin".into(),
            }),
        );
        let formatted = format_entry(&entry);
        assert_eq!(formatted, "#5 POLICY_UPDATE global by system");
    }

    #[test]
    fn format_entry_all_entry_types() {
        let types = vec![
            (1, "COMMIT"),
            (2, "ROLLBACK"),
            (3, "AUTO_CONVERGE"),
            (4, "DRIFT_DETECTED"),
            (5, "CAPABILITY_CHANGE"),
            (6, "POLICY_UPDATE"),
            (7, "BOOT_CONFIG"),
            (8, "EMERGENCY_ON"),
            (9, "EMERGENCY_OFF"),
            (10, "EXEC_LOG"),
            (11, "SHELL_SESSION"),
            (12, "SERVICE_LIFECYCLE"),
            (13, "PENDING_APPROVAL"),
            (99, "UNKNOWN"),
            (0, "UNKNOWN"),
        ];
        for (code, expected_name) in types {
            let entry = make_entry(1, code, None, None);
            let formatted = format_entry(&entry);
            assert!(
                formatted.contains(expected_name),
                "entry_type {code} should map to {expected_name}, got: {formatted}"
            );
        }
    }

    #[test]
    fn format_entry_scope_with_none_inner() {
        let entry = make_entry(1, 1, Some(ProtoScopeMsg { scope: None }), None);
        let formatted = format_entry(&entry);
        assert!(formatted.contains("global"));
    }

    // --- dispatch_connected tests (no gRPC connection) ---

    #[tokio::test]
    async fn dispatch_connected_unknown_tool_returns_none() {
        let connections = Connections {
            journal: None,
            agent: None,
            delegation: DelegationConfig::default(),
            mcp_token: None,
        };
        let result = dispatch_connected("nonexistent_tool", &json!({}), &connections).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn dispatch_connected_journal_tools_without_connection() {
        let connections = Connections {
            journal: None,
            agent: None,
            delegation: DelegationConfig::default(),
            mcp_token: None,
        };
        let journal_tools = vec![
            "pact_status",
            "pact_log",
            "pact_commit",
            "pact_rollback",
            "pact_diff",
            "pact_emergency",
            "pact_apply",
            "pact_query_fleet",
        ];

        for tool_name in journal_tools {
            let result = dispatch_connected(tool_name, &json!({}), &connections).await;
            let result = result.unwrap_or_else(|| panic!("{tool_name} should return Some"));
            assert!(result.is_error, "{tool_name} should be error without journal");
            assert!(
                result.content[0].text.contains("not connected to journal"),
                "{tool_name}: unexpected message: {}",
                result.content[0].text
            );
        }
    }

    #[tokio::test]
    async fn dispatch_connected_agent_tools_without_connection() {
        let connections = Connections {
            journal: None,
            agent: None,
            delegation: DelegationConfig::default(),
            mcp_token: None,
        };
        let agent_tools = vec!["pact_exec", "pact_cap", "pact_service_status"];

        for tool_name in agent_tools {
            let result = dispatch_connected(tool_name, &json!({}), &connections).await;
            let result = result.unwrap_or_else(|| panic!("{tool_name} should return Some"));
            assert!(result.is_error, "{tool_name} should be error without agent");
            assert!(
                result.content[0].text.contains("not connected to agent"),
                "{tool_name}: unexpected message: {}",
                result.content[0].text
            );
        }
    }

    #[tokio::test]
    async fn dispatch_tool_connected_backward_compat_returns_none_for_unknown() {
        // dispatch_tool_connected wraps dispatch_connected with journal-only connections
        // Create a fake channel that won't actually connect
        let connections = Connections {
            journal: None,
            agent: None,
            delegation: DelegationConfig::default(),
            mcp_token: None,
        };
        let result = dispatch_connected("no_such_tool", &json!({}), &connections).await;
        assert!(result.is_none());
    }

    // --- Argument validation tests via dispatch_connected (no connection) ---
    // These verify that tools that require args (commit, rollback, exec, apply, emergency)
    // produce proper error messages even without a gRPC connection.
    // Since connection is None, they return "not connected" before reaching arg validation.
    // But we can test the stub dispatch path in tools.rs for arg validation — covered there.

    // --- Emergency mode P8 restriction test ---
    // This is the only tool handler that has logic before the gRPC call.

    #[tokio::test]
    async fn dispatch_connected_emergency_start_without_connection() {
        let connections = Connections {
            journal: None,
            agent: None,
            delegation: DelegationConfig::default(),
            mcp_token: None,
        };
        let result = dispatch_connected(
            "pact_emergency",
            &json!({"action": "start", "reason": "test"}),
            &connections,
        )
        .await;
        let result = result.unwrap();
        // Without connection, we get "not connected" before reaching P8 check
        assert!(result.is_error);
        assert!(result.content[0].text.contains("not connected to journal"));
    }

    // --- Supercharged command dispatch tests (no lattice endpoint) ---

    #[tokio::test]
    async fn dispatch_connected_jobs_list_no_lattice() {
        let connections = Connections {
            journal: None,
            agent: None,
            delegation: DelegationConfig::default(),
            mcp_token: None,
        };
        let result =
            dispatch_connected("pact_jobs_list", &json!({"vcluster": "ml"}), &connections).await;
        let result = result.unwrap();
        assert!(result.is_error);
        assert!(result.content[0].text.contains("failed"));
    }

    #[tokio::test]
    async fn dispatch_connected_queue_status_no_lattice() {
        let connections = Connections {
            journal: None,
            agent: None,
            delegation: DelegationConfig::default(),
            mcp_token: None,
        };
        let result = dispatch_connected("pact_queue_status", &json!({}), &connections).await;
        let result = result.unwrap();
        assert!(result.is_error);
        assert!(result.content[0].text.contains("failed"));
    }

    #[tokio::test]
    async fn dispatch_connected_cluster_health_no_journal() {
        let connections = Connections {
            journal: None,
            agent: None,
            delegation: DelegationConfig::default(),
            mcp_token: None,
        };
        let result = dispatch_connected("pact_cluster_health", &json!({}), &connections).await;
        let result = result.unwrap();
        assert!(result.is_error);
        assert!(result.content[0].text.contains("not connected to journal"));
    }

    #[tokio::test]
    async fn dispatch_connected_system_health_no_journal() {
        let connections = Connections {
            journal: None,
            agent: None,
            delegation: DelegationConfig::default(),
            mcp_token: None,
        };
        let result = dispatch_connected("pact_system_health", &json!({}), &connections).await;
        let result = result.unwrap();
        assert!(result.is_error);
        assert!(result.content[0].text.contains("not connected to journal"));
    }

    #[tokio::test]
    async fn dispatch_connected_accounting_no_lattice() {
        let connections = Connections {
            journal: None,
            agent: None,
            delegation: DelegationConfig::default(),
            mcp_token: None,
        };
        let result =
            dispatch_connected("pact_accounting", &json!({"vcluster": "ml"}), &connections).await;
        let result = result.unwrap();
        assert!(result.is_error);
        assert!(result.content[0].text.contains("failed"));
    }

    // --- New delegation tool dispatch tests ---

    #[tokio::test]
    async fn dispatch_connected_undrain_no_journal() {
        let connections = Connections {
            journal: None,
            agent: None,
            delegation: DelegationConfig::default(),
            mcp_token: None,
        };
        let result =
            dispatch_connected("pact_undrain", &json!({"node": "n01"}), &connections).await;
        let result = result.unwrap();
        assert!(result.is_error);
        assert!(result.content[0].text.contains("not connected to journal"));
    }

    #[tokio::test]
    async fn dispatch_connected_dag_list_no_lattice() {
        let connections = Connections {
            journal: None,
            agent: None,
            delegation: DelegationConfig::default(),
            mcp_token: None,
        };
        let result = dispatch_connected("pact_dag_list", &json!({}), &connections).await;
        let result = result.unwrap();
        assert!(result.is_error);
        assert!(result.content[0].text.contains("failed"));
    }

    #[tokio::test]
    async fn dispatch_connected_dag_inspect_no_lattice() {
        let connections = Connections {
            journal: None,
            agent: None,
            delegation: DelegationConfig::default(),
            mcp_token: None,
        };
        let result =
            dispatch_connected("pact_dag_inspect", &json!({"dag_id": "d1"}), &connections).await;
        let result = result.unwrap();
        assert!(result.is_error);
        assert!(result.content[0].text.contains("failed"));
    }

    #[tokio::test]
    async fn dispatch_connected_budget_no_lattice() {
        let connections = Connections {
            journal: None,
            agent: None,
            delegation: DelegationConfig::default(),
            mcp_token: None,
        };
        let result =
            dispatch_connected("pact_budget", &json!({"tenant": "ml"}), &connections).await;
        let result = result.unwrap();
        assert!(result.is_error);
        assert!(result.content[0].text.contains("failed"));
    }

    #[tokio::test]
    async fn dispatch_connected_budget_missing_args() {
        let connections = Connections {
            journal: None,
            agent: None,
            delegation: DelegationConfig::default(),
            mcp_token: None,
        };
        let result = dispatch_connected("pact_budget", &json!({}), &connections).await;
        let result = result.unwrap();
        assert!(result.is_error);
        assert!(result.content[0].text.contains("specify tenant or user"));
    }

    #[tokio::test]
    async fn dispatch_connected_backup_create_no_journal() {
        let connections = Connections {
            journal: None,
            agent: None,
            delegation: DelegationConfig::default(),
            mcp_token: None,
        };
        let result =
            dispatch_connected("pact_backup_create", &json!({"path": "/tmp/b"}), &connections)
                .await;
        let result = result.unwrap();
        assert!(result.is_error);
        assert!(result.content[0].text.contains("not connected to journal"));
    }

    #[tokio::test]
    async fn dispatch_connected_backup_verify_no_lattice() {
        let connections = Connections {
            journal: None,
            agent: None,
            delegation: DelegationConfig::default(),
            mcp_token: None,
        };
        let result =
            dispatch_connected("pact_backup_verify", &json!({"path": "/tmp/b"}), &connections)
                .await;
        let result = result.unwrap();
        assert!(result.is_error);
        assert!(result.content[0].text.contains("failed"));
    }

    #[tokio::test]
    async fn dispatch_connected_nodes_list_no_lattice() {
        let connections = Connections {
            journal: None,
            agent: None,
            delegation: DelegationConfig::default(),
            mcp_token: None,
        };
        let result = dispatch_connected("pact_nodes_list", &json!({}), &connections).await;
        let result = result.unwrap();
        assert!(result.is_error);
        assert!(result.content[0].text.contains("failed"));
    }

    #[tokio::test]
    async fn dispatch_connected_node_inspect_no_lattice() {
        let connections = Connections {
            journal: None,
            agent: None,
            delegation: DelegationConfig::default(),
            mcp_token: None,
        };
        let result =
            dispatch_connected("pact_node_inspect", &json!({"node_id": "n01"}), &connections).await;
        let result = result.unwrap();
        assert!(result.is_error);
        assert!(result.content[0].text.contains("failed"));
    }
}
