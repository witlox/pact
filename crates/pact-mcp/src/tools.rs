//! MCP tool definitions for pact operations.
//!
//! Each tool maps to a pact CLI/gRPC operation. Tools are organized into:
//! - Read operations: status, diff, log, cap, service_status, query_fleet
//! - Write operations: commit, rollback, apply, exec
//! - Admin operations: emergency (restricted to human admins per P8)

use serde_json::json;

use crate::protocol::{tool_result, ToolCallResult, ToolDefinition};

/// Return all available MCP tools.
pub fn all_tools() -> Vec<ToolDefinition> {
    vec![
        pact_status(),
        pact_diff(),
        pact_log(),
        pact_commit(),
        pact_rollback(),
        pact_exec(),
        pact_apply(),
        pact_cap(),
        pact_service_status(),
        pact_query_fleet(),
        pact_emergency(),
        // Supercharged commands (read-only lattice delegation)
        pact_jobs_list(),
        pact_queue_status(),
        pact_cluster_health(),
        pact_system_health(),
        pact_accounting(),
        pact_services_list(),
        pact_services_lookup(),
        // New lattice delegation tools
        pact_undrain(),
        pact_dag_list(),
        pact_dag_inspect(),
        pact_budget(),
        pact_backup_create(),
        pact_backup_verify(),
        pact_nodes_list(),
        pact_node_inspect(),
    ]
}

fn pact_status() -> ToolDefinition {
    ToolDefinition {
        name: "pact_status".into(),
        description: "Query node or vCluster state, drift, and capabilities.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "node": {
                    "type": "string",
                    "description": "Node ID to query. Omit for all nodes in vCluster."
                },
                "vcluster": {
                    "type": "string",
                    "description": "vCluster scope."
                }
            }
        }),
    }
}

fn pact_diff() -> ToolDefinition {
    ToolDefinition {
        name: "pact_diff".into(),
        description: "Show declared vs actual state differences for a node.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "node": {
                    "type": "string",
                    "description": "Node ID to diff."
                },
                "committed": {
                    "type": "boolean",
                    "description": "Show committed node deltas not yet promoted.",
                    "default": false
                }
            }
        }),
    }
}

fn pact_log() -> ToolDefinition {
    ToolDefinition {
        name: "pact_log".into(),
        description: "Query configuration history from the journal.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "n": {
                    "type": "integer",
                    "description": "Number of entries to return.",
                    "default": 20
                },
                "scope": {
                    "type": "string",
                    "description": "Scope filter: 'node:X', 'vc:X', or 'global'."
                },
                "entry_types": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Filter by entry types (commit, rollback, drift_detected, etc.)."
                }
            }
        }),
    }
}

fn pact_commit() -> ToolDefinition {
    ToolDefinition {
        name: "pact_commit".into(),
        description: "Commit pending drift as a configuration entry.".into(),
        input_schema: json!({
            "type": "object",
            "required": ["message"],
            "properties": {
                "message": {
                    "type": "string",
                    "description": "Commit message describing the change."
                },
                "scope": {
                    "type": "string",
                    "description": "Scope: 'node:X' or 'vc:X'."
                }
            }
        }),
    }
}

fn pact_rollback() -> ToolDefinition {
    ToolDefinition {
        name: "pact_rollback".into(),
        description: "Roll back to a previous configuration state.".into(),
        input_schema: json!({
            "type": "object",
            "required": ["sequence"],
            "properties": {
                "sequence": {
                    "type": "integer",
                    "description": "Target sequence number to roll back to."
                }
            }
        }),
    }
}

fn pact_exec() -> ToolDefinition {
    ToolDefinition {
        name: "pact_exec".into(),
        description: "Run a whitelisted diagnostic command on a remote node.".into(),
        input_schema: json!({
            "type": "object",
            "required": ["node", "command"],
            "properties": {
                "node": {
                    "type": "string",
                    "description": "Target node ID."
                },
                "command": {
                    "type": "string",
                    "description": "Command to execute (must be whitelisted)."
                },
                "args": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Command arguments."
                }
            }
        }),
    }
}

fn pact_apply() -> ToolDefinition {
    ToolDefinition {
        name: "pact_apply".into(),
        description: "Apply a declarative config spec to a vCluster.".into(),
        input_schema: json!({
            "type": "object",
            "required": ["scope", "config", "message"],
            "properties": {
                "scope": {
                    "type": "string",
                    "description": "Target scope: 'vc:X'."
                },
                "config": {
                    "type": "object",
                    "description": "Configuration object to apply."
                },
                "message": {
                    "type": "string",
                    "description": "Description of the change."
                }
            }
        }),
    }
}

fn pact_cap() -> ToolDefinition {
    ToolDefinition {
        name: "pact_cap".into(),
        description: "Show node hardware capability report (GPUs, memory, network, storage)."
            .into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "node": {
                    "type": "string",
                    "description": "Node ID. Omit for all nodes."
                }
            }
        }),
    }
}

fn pact_service_status() -> ToolDefinition {
    ToolDefinition {
        name: "pact_service_status".into(),
        description: "Query service health across nodes.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "node": {
                    "type": "string",
                    "description": "Node ID. Omit for all nodes."
                },
                "service": {
                    "type": "string",
                    "description": "Service name. Omit for all services."
                }
            }
        }),
    }
}

fn pact_query_fleet() -> ToolDefinition {
    ToolDefinition {
        name: "pact_query_fleet".into(),
        description: "Fleet-wide health query with capability filtering.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "vcluster": {
                    "type": "string",
                    "description": "vCluster scope."
                },
                "capability_filter": {
                    "type": "string",
                    "description": "Filter expression (e.g. 'gpu_health=degraded')."
                },
                "config_state": {
                    "type": "string",
                    "description": "Filter by config state (committed, drifted, emergency)."
                }
            }
        }),
    }
}

fn pact_emergency() -> ToolDefinition {
    ToolDefinition {
        name: "pact_emergency".into(),
        description:
            "Start or end emergency mode. Note: AI agents are typically restricted from this (P8)."
                .into(),
        input_schema: json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["start", "end"],
                    "description": "Emergency action."
                },
                "reason": {
                    "type": "string",
                    "description": "Reason for emergency (required for start)."
                },
                "force": {
                    "type": "boolean",
                    "description": "Force-end another admin's emergency.",
                    "default": false
                }
            }
        }),
    }
}

// --- Supercharged command tool definitions ---

fn pact_jobs_list() -> ToolDefinition {
    ToolDefinition {
        name: "pact_jobs_list".into(),
        description: "List running job allocations from the lattice scheduler.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "node": {
                    "type": "string",
                    "description": "Filter by node ID."
                },
                "vcluster": {
                    "type": "string",
                    "description": "Filter by vCluster."
                }
            }
        }),
    }
}

fn pact_queue_status() -> ToolDefinition {
    ToolDefinition {
        name: "pact_queue_status".into(),
        description: "Show scheduling queue depth and status from the lattice scheduler.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "vcluster": {
                    "type": "string",
                    "description": "vCluster to query queue for. Defaults to 'default'."
                }
            }
        }),
    }
}

fn pact_cluster_health() -> ToolDefinition {
    ToolDefinition {
        name: "pact_cluster_health".into(),
        description: "Combined cluster health: pact journal Raft status and lattice Raft status."
            .into(),
        input_schema: json!({
            "type": "object",
            "properties": {}
        }),
    }
}

fn pact_system_health() -> ToolDefinition {
    ToolDefinition {
        name: "pact_system_health".into(),
        description: "Combined system health check across pact journal and lattice scheduler."
            .into(),
        input_schema: json!({
            "type": "object",
            "properties": {}
        }),
    }
}

fn pact_accounting() -> ToolDefinition {
    ToolDefinition {
        name: "pact_accounting".into(),
        description: "Resource usage accounting (CPU hours, GPU hours, storage) from lattice."
            .into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "vcluster": {
                    "type": "string",
                    "description": "vCluster (maps to tenant) to query accounting for."
                }
            }
        }),
    }
}

fn pact_services_list() -> ToolDefinition {
    ToolDefinition {
        name: "pact_services_list".into(),
        description: "List registered services from the lattice service registry.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {}
        }),
    }
}

fn pact_services_lookup() -> ToolDefinition {
    ToolDefinition {
        name: "pact_services_lookup".into(),
        description: "Look up endpoints for a named service in the lattice service registry."
            .into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Service name to look up."
                }
            },
            "required": ["name"]
        }),
    }
}

// --- New lattice delegation tool definitions ---

fn pact_undrain() -> ToolDefinition {
    ToolDefinition {
        name: "pact_undrain".into(),
        description: "Cancel a drain on a node, returning it to Ready state (delegates to lattice)."
            .into(),
        input_schema: json!({
            "type": "object",
            "required": ["node"],
            "properties": {
                "node": {
                    "type": "string",
                    "description": "Node ID to undrain."
                }
            }
        }),
    }
}

fn pact_dag_list() -> ToolDefinition {
    ToolDefinition {
        name: "pact_dag_list".into(),
        description: "List DAG workflows from the lattice scheduler.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "tenant": { "type": "string", "description": "Filter by tenant." },
                "state": { "type": "string", "description": "Filter by state (running, completed, failed, cancelled)." },
                "limit": { "type": "integer", "description": "Max results (default 50).", "default": 50 }
            }
        }),
    }
}

fn pact_dag_inspect() -> ToolDefinition {
    ToolDefinition {
        name: "pact_dag_inspect".into(),
        description: "Inspect a DAG workflow — show steps and allocation status.".into(),
        input_schema: json!({
            "type": "object",
            "required": ["dag_id"],
            "properties": {
                "dag_id": { "type": "string", "description": "DAG ID to inspect." }
            }
        }),
    }
}

fn pact_budget() -> ToolDefinition {
    ToolDefinition {
        name: "pact_budget".into(),
        description: "Query budget/usage for a tenant or user from lattice.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "tenant": { "type": "string", "description": "Tenant ID to query budget for." },
                "user": { "type": "string", "description": "User ID to query budget for (across all tenants)." },
                "days": { "type": "integer", "description": "Rolling window in days (default 90).", "default": 90 }
            }
        }),
    }
}

fn pact_backup_create() -> ToolDefinition {
    ToolDefinition {
        name: "pact_backup_create".into(),
        description: "Create a backup of lattice Raft state. Requires pact-platform-admin.".into(),
        input_schema: json!({
            "type": "object",
            "required": ["path"],
            "properties": {
                "path": { "type": "string", "description": "Backup file path." }
            }
        }),
    }
}

fn pact_backup_verify() -> ToolDefinition {
    ToolDefinition {
        name: "pact_backup_verify".into(),
        description: "Verify integrity of a lattice backup file.".into(),
        input_schema: json!({
            "type": "object",
            "required": ["path"],
            "properties": {
                "path": { "type": "string", "description": "Backup file path to verify." }
            }
        }),
    }
}

fn pact_nodes_list() -> ToolDefinition {
    ToolDefinition {
        name: "pact_nodes_list".into(),
        description: "List lattice nodes with state, GPU, and ownership info.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "state": { "type": "string", "description": "Filter by state (ready, draining, drained, down)." },
                "vcluster": { "type": "string", "description": "Filter by vCluster." },
                "limit": { "type": "integer", "description": "Max results (default 100).", "default": 100 }
            }
        }),
    }
}

fn pact_node_inspect() -> ToolDefinition {
    ToolDefinition {
        name: "pact_node_inspect".into(),
        description: "Inspect a single lattice node — hardware, ownership, allocations.".into(),
        input_schema: json!({
            "type": "object",
            "required": ["node_id"],
            "properties": {
                "node_id": { "type": "string", "description": "Node ID to inspect." }
            }
        }),
    }
}

/// Dispatch a tool call to the appropriate handler.
pub fn dispatch_tool(name: &str, arguments: &serde_json::Value) -> ToolCallResult {
    match name {
        "pact_status" => handle_status(arguments),
        "pact_diff" => handle_diff(arguments),
        "pact_log" => handle_log(arguments),
        "pact_commit" => handle_commit(arguments),
        "pact_rollback" => handle_rollback(arguments),
        "pact_exec" => handle_exec(arguments),
        "pact_apply" => handle_apply(arguments),
        "pact_cap" => handle_cap(arguments),
        "pact_service_status" => handle_service_status(arguments),
        "pact_query_fleet" => handle_query_fleet(arguments),
        "pact_emergency" => handle_emergency(arguments),
        "pact_jobs_list" => handle_jobs_list(arguments),
        "pact_queue_status" => handle_queue_status(arguments),
        "pact_cluster_health" => handle_cluster_health(arguments),
        "pact_system_health" => handle_system_health(arguments),
        "pact_accounting" => handle_accounting(arguments),
        "pact_services_list" => handle_services_list(arguments),
        "pact_services_lookup" => handle_services_lookup(arguments),
        "pact_undrain" => handle_undrain(arguments),
        "pact_dag_list" => handle_dag_list(arguments),
        "pact_dag_inspect" => handle_dag_inspect(arguments),
        "pact_budget" => handle_budget(arguments),
        "pact_backup_create" => handle_backup_create(arguments),
        "pact_backup_verify" => handle_backup_verify(arguments),
        "pact_nodes_list" => handle_nodes_list(arguments),
        "pact_node_inspect" => handle_node_inspect(arguments),
        _ => tool_result(format!("Unknown tool: {name}"), true),
    }
}

// Tool handlers — these will call gRPC services when connected.
// Currently return descriptive stub responses.

fn handle_status(args: &serde_json::Value) -> ToolCallResult {
    let node = args.get("node").and_then(|v| v.as_str()).unwrap_or("all");
    let vc = args.get("vcluster").and_then(|v| v.as_str()).unwrap_or("default");
    tool_result(format!("Status query: node={node}, vcluster={vc} (gRPC client required)"), false)
}

fn handle_diff(args: &serde_json::Value) -> ToolCallResult {
    let node = args.get("node").and_then(|v| v.as_str()).unwrap_or("current");
    let committed = args.get("committed").and_then(serde_json::Value::as_bool).unwrap_or(false);
    tool_result(
        format!("Diff query: node={node}, committed={committed} (gRPC client required)"),
        false,
    )
}

fn handle_log(args: &serde_json::Value) -> ToolCallResult {
    let n = args.get("n").and_then(serde_json::Value::as_u64).unwrap_or(20);
    let scope = args.get("scope").and_then(|v| v.as_str()).unwrap_or("all");
    tool_result(format!("Log query: n={n}, scope={scope} (gRPC client required)"), false)
}

fn handle_commit(args: &serde_json::Value) -> ToolCallResult {
    let message = match args.get("message").and_then(|v| v.as_str()) {
        Some(m) => m,
        None => return tool_result("Error: commit message required", true),
    };
    tool_result(format!("Commit: message={message:?} (gRPC client required)"), false)
}

fn handle_rollback(args: &serde_json::Value) -> ToolCallResult {
    let seq = match args.get("sequence").and_then(serde_json::Value::as_u64) {
        Some(s) => s,
        None => return tool_result("Error: sequence number required", true),
    };
    tool_result(format!("Rollback: target_seq={seq} (gRPC client required)"), false)
}

fn handle_exec(args: &serde_json::Value) -> ToolCallResult {
    let node = match args.get("node").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return tool_result("Error: node ID required", true),
    };
    let command = match args.get("command").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return tool_result("Error: command required", true),
    };
    tool_result(format!("Exec: node={node}, command={command:?} (gRPC client required)"), false)
}

fn handle_apply(args: &serde_json::Value) -> ToolCallResult {
    let scope = match args.get("scope").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return tool_result("Error: scope required", true),
    };
    let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("auto-apply");
    tool_result(format!("Apply: scope={scope}, message={message:?} (gRPC client required)"), false)
}

fn handle_cap(args: &serde_json::Value) -> ToolCallResult {
    let node = args.get("node").and_then(|v| v.as_str()).unwrap_or("all");
    tool_result(format!("Capability report: node={node} (gRPC client required)"), false)
}

fn handle_service_status(args: &serde_json::Value) -> ToolCallResult {
    let node = args.get("node").and_then(|v| v.as_str()).unwrap_or("all");
    let service = args.get("service").and_then(|v| v.as_str()).unwrap_or("all");
    tool_result(
        format!("Service status: node={node}, service={service} (gRPC client required)"),
        false,
    )
}

fn handle_query_fleet(args: &serde_json::Value) -> ToolCallResult {
    let vc = args.get("vcluster").and_then(|v| v.as_str()).unwrap_or("all");
    let filter = args.get("capability_filter").and_then(|v| v.as_str()).unwrap_or("none");
    tool_result(
        format!("Fleet query: vcluster={vc}, filter={filter} (gRPC client required)"),
        false,
    )
}

fn handle_emergency(args: &serde_json::Value) -> ToolCallResult {
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

    tool_result(format!("Emergency: action={action} (gRPC client required)"), false)
}

// --- Supercharged command stub handlers ---

fn handle_jobs_list(args: &serde_json::Value) -> ToolCallResult {
    let node = args.get("node").and_then(|v| v.as_str()).unwrap_or("all");
    let vc = args.get("vcluster").and_then(|v| v.as_str()).unwrap_or("all");
    tool_result(format!("Jobs list: node={node}, vcluster={vc} (lattice client required)"), false)
}

fn handle_queue_status(args: &serde_json::Value) -> ToolCallResult {
    let vc = args.get("vcluster").and_then(|v| v.as_str()).unwrap_or("default");
    tool_result(format!("Queue status: vcluster={vc} (lattice client required)"), false)
}

fn handle_cluster_health(_args: &serde_json::Value) -> ToolCallResult {
    tool_result("Cluster health: (gRPC + lattice client required)".to_string(), false)
}

fn handle_system_health(_args: &serde_json::Value) -> ToolCallResult {
    tool_result("System health: (gRPC + lattice client required)".to_string(), false)
}

fn handle_accounting(args: &serde_json::Value) -> ToolCallResult {
    let vc = args.get("vcluster").and_then(|v| v.as_str()).unwrap_or("all");
    tool_result(format!("Accounting: vcluster={vc} (lattice client required)"), false)
}

fn handle_services_list(_args: &serde_json::Value) -> ToolCallResult {
    tool_result("Services list (lattice client required)".to_string(), false)
}

fn handle_services_lookup(args: &serde_json::Value) -> ToolCallResult {
    let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("unknown");
    tool_result(format!("Service lookup: {name} (lattice client required)"), false)
}

// --- New lattice delegation stub handlers ---

fn handle_undrain(args: &serde_json::Value) -> ToolCallResult {
    let node = match args.get("node").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => return tool_result("Error: node ID required", true),
    };
    tool_result(format!("Undrain: node={node} (journal + lattice client required)"), false)
}

fn handle_dag_list(args: &serde_json::Value) -> ToolCallResult {
    let tenant = args.get("tenant").and_then(|v| v.as_str()).unwrap_or("all");
    let state = args.get("state").and_then(|v| v.as_str()).unwrap_or("all");
    tool_result(
        format!("DAG list: tenant={tenant}, state={state} (lattice client required)"),
        false,
    )
}

fn handle_dag_inspect(args: &serde_json::Value) -> ToolCallResult {
    let dag_id = match args.get("dag_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return tool_result("Error: dag_id required", true),
    };
    tool_result(format!("DAG inspect: id={dag_id} (lattice client required)"), false)
}

fn handle_budget(args: &serde_json::Value) -> ToolCallResult {
    let tenant = args.get("tenant").and_then(|v| v.as_str());
    let user = args.get("user").and_then(|v| v.as_str());
    match (tenant, user) {
        (Some(t), _) => tool_result(format!("Budget: tenant={t} (lattice client required)"), false),
        (_, Some(u)) => tool_result(format!("Budget: user={u} (lattice client required)"), false),
        _ => tool_result("Error: specify tenant or user", true),
    }
}

fn handle_backup_create(args: &serde_json::Value) -> ToolCallResult {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return tool_result("Error: path required", true),
    };
    tool_result(
        format!("Backup create: path={path} (journal + lattice client required)"),
        false,
    )
}

fn handle_backup_verify(args: &serde_json::Value) -> ToolCallResult {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return tool_result("Error: path required", true),
    };
    tool_result(format!("Backup verify: path={path} (lattice client required)"), false)
}

fn handle_nodes_list(args: &serde_json::Value) -> ToolCallResult {
    let state = args.get("state").and_then(|v| v.as_str()).unwrap_or("all");
    let vc = args.get("vcluster").and_then(|v| v.as_str()).unwrap_or("all");
    tool_result(
        format!("Nodes list: state={state}, vcluster={vc} (lattice client required)"),
        false,
    )
}

fn handle_node_inspect(args: &serde_json::Value) -> ToolCallResult {
    let node_id = match args.get("node_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return tool_result("Error: node_id required", true),
    };
    tool_result(format!("Node inspect: id={node_id} (lattice client required)"), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_tools_count() {
        let tools = all_tools();
        assert_eq!(tools.len(), 26);
    }

    #[test]
    fn all_tools_have_unique_names() {
        let tools = all_tools();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(names.len(), sorted.len(), "duplicate tool names");
    }

    #[test]
    fn all_tools_have_descriptions() {
        for tool in all_tools() {
            assert!(!tool.description.is_empty(), "tool {} has no description", tool.name);
        }
    }

    #[test]
    fn all_tools_have_valid_schemas() {
        for tool in all_tools() {
            assert!(tool.input_schema.is_object(), "tool {} schema is not an object", tool.name);
            assert_eq!(
                tool.input_schema["type"], "object",
                "tool {} schema type is not 'object'",
                tool.name
            );
        }
    }

    #[test]
    fn dispatch_unknown_tool() {
        let result = dispatch_tool("nonexistent", &json!({}));
        assert!(result.is_error);
        assert!(result.content[0].text.contains("Unknown tool"));
    }

    #[test]
    fn dispatch_pact_status() {
        let result = dispatch_tool("pact_status", &json!({"node": "node042", "vcluster": "ml"}));
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("node042"));
    }

    #[test]
    fn dispatch_pact_exec() {
        let result =
            dispatch_tool("pact_exec", &json!({"node": "node042", "command": "nvidia-smi"}));
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("nvidia-smi"));
    }

    #[test]
    fn dispatch_pact_exec_missing_node() {
        let result = dispatch_tool("pact_exec", &json!({"command": "hostname"}));
        assert!(result.is_error);
        assert!(result.content[0].text.contains("node ID required"));
    }

    #[test]
    fn dispatch_pact_exec_missing_command() {
        let result = dispatch_tool("pact_exec", &json!({"node": "n01"}));
        assert!(result.is_error);
        assert!(result.content[0].text.contains("command required"));
    }

    #[test]
    fn dispatch_pact_commit() {
        let result = dispatch_tool("pact_commit", &json!({"message": "fix GPU config"}));
        assert!(!result.is_error);
    }

    #[test]
    fn dispatch_pact_commit_missing_message() {
        let result = dispatch_tool("pact_commit", &json!({}));
        assert!(result.is_error);
    }

    #[test]
    fn dispatch_pact_rollback() {
        let result = dispatch_tool("pact_rollback", &json!({"sequence": 4810}));
        assert!(!result.is_error);
    }

    #[test]
    fn dispatch_pact_rollback_missing_sequence() {
        let result = dispatch_tool("pact_rollback", &json!({}));
        assert!(result.is_error);
    }

    #[test]
    fn dispatch_pact_emergency_start_blocked_p8() {
        let result =
            dispatch_tool("pact_emergency", &json!({"action": "start", "reason": "gpu failure"}));
        assert!(result.is_error);
        assert!(result.content[0].text.contains("P8"));
    }

    #[test]
    fn dispatch_pact_emergency_end_allowed() {
        let result = dispatch_tool("pact_emergency", &json!({"action": "end"}));
        assert!(!result.is_error);
    }

    #[test]
    fn dispatch_pact_emergency_missing_action() {
        let result = dispatch_tool("pact_emergency", &json!({}));
        assert!(result.is_error);
    }

    #[test]
    fn dispatch_pact_diff() {
        let result = dispatch_tool("pact_diff", &json!({"node": "node042", "committed": true}));
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("committed=true"));
    }

    #[test]
    fn dispatch_pact_log() {
        let result = dispatch_tool("pact_log", &json!({"n": 10, "scope": "vc:ml"}));
        assert!(!result.is_error);
    }

    #[test]
    fn dispatch_pact_cap() {
        let result = dispatch_tool("pact_cap", &json!({"node": "node042"}));
        assert!(!result.is_error);
    }

    #[test]
    fn dispatch_pact_service_status() {
        let result =
            dispatch_tool("pact_service_status", &json!({"node": "node042", "service": "chronyd"}));
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("chronyd"));
    }

    #[test]
    fn dispatch_pact_query_fleet() {
        let result = dispatch_tool(
            "pact_query_fleet",
            &json!({"vcluster": "ml-training", "capability_filter": "gpu_health=degraded"}),
        );
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("gpu_health=degraded"));
    }

    #[test]
    fn dispatch_pact_apply() {
        let result = dispatch_tool(
            "pact_apply",
            &json!({"scope": "vc:ml", "config": {"sysctl": {}}, "message": "auto-fix"}),
        );
        assert!(!result.is_error);
    }

    #[test]
    fn dispatch_pact_apply_missing_scope() {
        let result = dispatch_tool("pact_apply", &json!({"config": {}}));
        assert!(result.is_error);
    }

    #[test]
    fn tool_names_start_with_pact() {
        for tool in all_tools() {
            assert!(tool.name.starts_with("pact_"), "tool {} doesn't start with pact_", tool.name);
        }
    }

    // --- Supercharged command tests ---

    #[test]
    fn dispatch_pact_jobs_list() {
        let result = dispatch_tool("pact_jobs_list", &json!({"node": "node042", "vcluster": "ml"}));
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("node042"));
        assert!(result.content[0].text.contains("ml"));
    }

    #[test]
    fn dispatch_pact_jobs_list_defaults() {
        let result = dispatch_tool("pact_jobs_list", &json!({}));
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("all"));
    }

    #[test]
    fn dispatch_pact_queue_status() {
        let result = dispatch_tool("pact_queue_status", &json!({"vcluster": "ml-training"}));
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("ml-training"));
    }

    #[test]
    fn dispatch_pact_queue_status_default() {
        let result = dispatch_tool("pact_queue_status", &json!({}));
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("default"));
    }

    #[test]
    fn dispatch_pact_cluster_health() {
        let result = dispatch_tool("pact_cluster_health", &json!({}));
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("Cluster health"));
    }

    #[test]
    fn dispatch_pact_system_health() {
        let result = dispatch_tool("pact_system_health", &json!({}));
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("System health"));
    }

    #[test]
    fn dispatch_pact_accounting() {
        let result = dispatch_tool("pact_accounting", &json!({"vcluster": "ml"}));
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("ml"));
    }

    #[test]
    fn dispatch_pact_accounting_default() {
        let result = dispatch_tool("pact_accounting", &json!({}));
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("all"));
    }

    #[test]
    fn supercharged_tools_have_definitions() {
        let tools = all_tools();
        let supercharged = [
            "pact_jobs_list",
            "pact_queue_status",
            "pact_cluster_health",
            "pact_system_health",
            "pact_accounting",
        ];
        for name in &supercharged {
            assert!(
                tools.iter().any(|t| t.name == *name),
                "supercharged tool {name} not found in all_tools()"
            );
        }
    }

    #[test]
    fn new_delegation_tools_have_definitions() {
        let tools = all_tools();
        let new_tools = [
            "pact_undrain",
            "pact_dag_list",
            "pact_dag_inspect",
            "pact_budget",
            "pact_backup_create",
            "pact_backup_verify",
            "pact_nodes_list",
            "pact_node_inspect",
        ];
        for name in &new_tools {
            assert!(
                tools.iter().any(|t| t.name == *name),
                "new tool {name} not found in all_tools()"
            );
        }
    }

    #[test]
    fn required_fields_present_in_schemas() {
        let tools = all_tools();

        // commit requires message
        let commit = tools.iter().find(|t| t.name == "pact_commit").unwrap();
        let required = commit.input_schema.get("required").unwrap().as_array().unwrap();
        assert!(required.iter().any(|r| r == "message"));

        // exec requires node and command
        let exec = tools.iter().find(|t| t.name == "pact_exec").unwrap();
        let required = exec.input_schema.get("required").unwrap().as_array().unwrap();
        assert!(required.iter().any(|r| r == "node"));
        assert!(required.iter().any(|r| r == "command"));

        // rollback requires sequence
        let rollback = tools.iter().find(|t| t.name == "pact_rollback").unwrap();
        let required = rollback.input_schema.get("required").unwrap().as_array().unwrap();
        assert!(required.iter().any(|r| r == "sequence"));

        // undrain requires node
        let undrain = tools.iter().find(|t| t.name == "pact_undrain").unwrap();
        let required = undrain.input_schema.get("required").unwrap().as_array().unwrap();
        assert!(required.iter().any(|r| r == "node"));

        // dag_inspect requires dag_id
        let dag = tools.iter().find(|t| t.name == "pact_dag_inspect").unwrap();
        let required = dag.input_schema.get("required").unwrap().as_array().unwrap();
        assert!(required.iter().any(|r| r == "dag_id"));

        // backup_create requires path
        let backup = tools.iter().find(|t| t.name == "pact_backup_create").unwrap();
        let required = backup.input_schema.get("required").unwrap().as_array().unwrap();
        assert!(required.iter().any(|r| r == "path"));

        // node_inspect requires node_id
        let node = tools.iter().find(|t| t.name == "pact_node_inspect").unwrap();
        let required = node.input_schema.get("required").unwrap().as_array().unwrap();
        assert!(required.iter().any(|r| r == "node_id"));
    }

    // --- New delegation tool stub tests ---

    #[test]
    fn dispatch_pact_undrain() {
        let result = dispatch_tool("pact_undrain", &json!({"node": "node042"}));
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("node042"));
    }

    #[test]
    fn dispatch_pact_undrain_missing_node() {
        let result = dispatch_tool("pact_undrain", &json!({}));
        assert!(result.is_error);
        assert!(result.content[0].text.contains("node ID required"));
    }

    #[test]
    fn dispatch_pact_dag_list() {
        let result = dispatch_tool("pact_dag_list", &json!({"tenant": "ml-team", "state": "running"}));
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("ml-team"));
    }

    #[test]
    fn dispatch_pact_dag_inspect() {
        let result = dispatch_tool("pact_dag_inspect", &json!({"dag_id": "dag-123"}));
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("dag-123"));
    }

    #[test]
    fn dispatch_pact_dag_inspect_missing_id() {
        let result = dispatch_tool("pact_dag_inspect", &json!({}));
        assert!(result.is_error);
        assert!(result.content[0].text.contains("dag_id required"));
    }

    #[test]
    fn dispatch_pact_budget_tenant() {
        let result = dispatch_tool("pact_budget", &json!({"tenant": "ml-team"}));
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("ml-team"));
    }

    #[test]
    fn dispatch_pact_budget_user() {
        let result = dispatch_tool("pact_budget", &json!({"user": "alice"}));
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("alice"));
    }

    #[test]
    fn dispatch_pact_budget_missing_both() {
        let result = dispatch_tool("pact_budget", &json!({}));
        assert!(result.is_error);
        assert!(result.content[0].text.contains("specify tenant or user"));
    }

    #[test]
    fn dispatch_pact_backup_create() {
        let result = dispatch_tool("pact_backup_create", &json!({"path": "/tmp/backup.bin"}));
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("/tmp/backup.bin"));
    }

    #[test]
    fn dispatch_pact_backup_create_missing_path() {
        let result = dispatch_tool("pact_backup_create", &json!({}));
        assert!(result.is_error);
        assert!(result.content[0].text.contains("path required"));
    }

    #[test]
    fn dispatch_pact_backup_verify() {
        let result = dispatch_tool("pact_backup_verify", &json!({"path": "/tmp/backup.bin"}));
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("/tmp/backup.bin"));
    }

    #[test]
    fn dispatch_pact_nodes_list() {
        let result =
            dispatch_tool("pact_nodes_list", &json!({"state": "ready", "vcluster": "ml"}));
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("ready"));
    }

    #[test]
    fn dispatch_pact_node_inspect() {
        let result = dispatch_tool("pact_node_inspect", &json!({"node_id": "node042"}));
        assert!(!result.is_error);
        assert!(result.content[0].text.contains("node042"));
    }

    #[test]
    fn dispatch_pact_node_inspect_missing_id() {
        let result = dispatch_tool("pact_node_inspect", &json!({}));
        assert!(result.is_error);
        assert!(result.content[0].text.contains("node_id required"));
    }
}
