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
        _ => tool_result(format!("Unknown tool: {}", name), true),
    }
}

// Tool handlers — these will call gRPC services when connected.
// Currently return descriptive stub responses.

fn handle_status(args: &serde_json::Value) -> ToolCallResult {
    let node = args.get("node").and_then(|v| v.as_str()).unwrap_or("all");
    let vc = args.get("vcluster").and_then(|v| v.as_str()).unwrap_or("default");
    tool_result(
        format!("Status query: node={}, vcluster={} (gRPC client required)", node, vc),
        false,
    )
}

fn handle_diff(args: &serde_json::Value) -> ToolCallResult {
    let node = args.get("node").and_then(|v| v.as_str()).unwrap_or("current");
    let committed = args.get("committed").and_then(|v| v.as_bool()).unwrap_or(false);
    tool_result(
        format!("Diff query: node={}, committed={} (gRPC client required)", node, committed),
        false,
    )
}

fn handle_log(args: &serde_json::Value) -> ToolCallResult {
    let n = args.get("n").and_then(|v| v.as_u64()).unwrap_or(20);
    let scope = args.get("scope").and_then(|v| v.as_str()).unwrap_or("all");
    tool_result(format!("Log query: n={}, scope={} (gRPC client required)", n, scope), false)
}

fn handle_commit(args: &serde_json::Value) -> ToolCallResult {
    let message = match args.get("message").and_then(|v| v.as_str()) {
        Some(m) => m,
        None => return tool_result("Error: commit message required", true),
    };
    tool_result(format!("Commit: message={:?} (gRPC client required)", message), false)
}

fn handle_rollback(args: &serde_json::Value) -> ToolCallResult {
    let seq = match args.get("sequence").and_then(|v| v.as_u64()) {
        Some(s) => s,
        None => return tool_result("Error: sequence number required", true),
    };
    tool_result(format!("Rollback: target_seq={} (gRPC client required)", seq), false)
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
    tool_result(format!("Exec: node={}, command={:?} (gRPC client required)", node, command), false)
}

fn handle_apply(args: &serde_json::Value) -> ToolCallResult {
    let scope = match args.get("scope").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return tool_result("Error: scope required", true),
    };
    let message = args.get("message").and_then(|v| v.as_str()).unwrap_or("auto-apply");
    tool_result(
        format!("Apply: scope={}, message={:?} (gRPC client required)", scope, message),
        false,
    )
}

fn handle_cap(args: &serde_json::Value) -> ToolCallResult {
    let node = args.get("node").and_then(|v| v.as_str()).unwrap_or("all");
    tool_result(format!("Capability report: node={} (gRPC client required)", node), false)
}

fn handle_service_status(args: &serde_json::Value) -> ToolCallResult {
    let node = args.get("node").and_then(|v| v.as_str()).unwrap_or("all");
    let service = args.get("service").and_then(|v| v.as_str()).unwrap_or("all");
    tool_result(
        format!("Service status: node={}, service={} (gRPC client required)", node, service),
        false,
    )
}

fn handle_query_fleet(args: &serde_json::Value) -> ToolCallResult {
    let vc = args.get("vcluster").and_then(|v| v.as_str()).unwrap_or("all");
    let filter = args.get("capability_filter").and_then(|v| v.as_str()).unwrap_or("none");
    tool_result(
        format!("Fleet query: vcluster={}, filter={} (gRPC client required)", vc, filter),
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

    tool_result(format!("Emergency: action={} (gRPC client required)", action), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_tools_count() {
        let tools = all_tools();
        assert_eq!(tools.len(), 11);
    }

    #[test]
    fn all_tools_have_unique_names() {
        let tools = all_tools();
        let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        let mut sorted = names.clone();
        sorted.sort();
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
    }
}
