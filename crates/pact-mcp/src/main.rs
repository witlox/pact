//! pact-mcp — MCP server for AI agent tool-use.
//!
//! Communicates via JSON-RPC 2.0 over stdio (stdin/stdout).
//! Connects to journal gRPC when available; falls back to stubs.
//! See `docs/architecture/agentic-api.md` for tool reference.

use std::io::{self, BufRead, Write};

use tonic::transport::Channel;
use tracing::warn;

use pact_common::config::DelegationConfig;
use pact_mcp::connected;
use pact_mcp::protocol::{
    self, error_codes, error_response, success_response, JsonRpcRequest, ServerCapabilities,
    ServerInfo, ToolCallParams, ToolsCapability,
};
use pact_mcp::tools;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::WARN.into()),
        )
        .with_writer(io::stderr)
        .init();

    // Try to connect to journal (from PACT_ENDPOINT env or default)
    let endpoint =
        std::env::var("PACT_ENDPOINT").unwrap_or_else(|_| "http://localhost:9443".to_string());
    let channel = try_connect(&endpoint, "journal").await;

    // Try to connect to agent (from PACT_AGENT_ENDPOINT env or default)
    let agent_endpoint = std::env::var("PACT_AGENT_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:9445".to_string());
    let agent_channel = try_connect(&agent_endpoint, "agent").await;

    // Build delegation config from environment for supercharged (lattice) commands
    let delegation = DelegationConfig {
        lattice_endpoint: std::env::var("PACT_LATTICE_ENDPOINT").ok(),
        lattice_token: std::env::var("PACT_LATTICE_TOKEN").ok(),
        ..DelegationConfig::default()
    };

    let connections = connected::Connections { journal: channel, agent: agent_channel, delegation };

    eprintln!("pact-mcp: starting MCP server on stdio");

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("stdin error: {e}");
                break;
            }
        };

        if line.trim().is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(&line) {
            Ok(req) => req,
            Err(e) => {
                let resp = error_response(
                    serde_json::Value::Null,
                    error_codes::PARSE_ERROR,
                    e.to_string(),
                );
                let _ = writeln!(stdout, "{}", serde_json::to_string(&resp).unwrap());
                continue;
            }
        };

        let response = handle_request(&request, &connections).await;
        let _ = writeln!(stdout, "{}", serde_json::to_string(&response).unwrap());
        let _ = stdout.flush();
    }
}

async fn try_connect(endpoint: &str, label: &str) -> Option<Channel> {
    match Channel::from_shared(endpoint.to_string()) {
        Ok(ch) => match ch.connect().await {
            Ok(channel) => {
                eprintln!("pact-mcp: connected to {label} at {endpoint}");
                Some(channel)
            }
            Err(e) => {
                warn!(error = %e, label, "Endpoint unreachable");
                eprintln!("pact-mcp: {label} unreachable at {endpoint}");
                None
            }
        },
        Err(e) => {
            warn!(error = %e, label, "Invalid endpoint");
            None
        }
    }
}

async fn handle_request(
    request: &JsonRpcRequest,
    connections: &connected::Connections,
) -> protocol::JsonRpcResponse {
    match request.method.as_str() {
        "initialize" => {
            let result = serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": ServerCapabilities {
                    tools: ToolsCapability { list_changed: false },
                },
                "serverInfo": ServerInfo {
                    name: "pact-mcp".into(),
                    version: env!("CARGO_PKG_VERSION").into(),
                },
            });
            success_response(request.id.clone(), result)
        }

        "tools/list" => {
            let tool_list = tools::all_tools();
            success_response(request.id.clone(), serde_json::json!({"tools": tool_list}))
        }

        "tools/call" => {
            let params: ToolCallParams = match serde_json::from_value(request.params.clone()) {
                Ok(p) => p,
                Err(e) => {
                    return error_response(
                        request.id.clone(),
                        error_codes::INVALID_PARAMS,
                        format!("invalid tool call params: {e}"),
                    );
                }
            };

            // Try connected dispatch first, fall back to stubs
            let result =
                match connected::dispatch_connected(&params.name, &params.arguments, connections)
                    .await
                {
                    Some(r) => r,
                    None => tools::dispatch_tool(&params.name, &params.arguments),
                };

            success_response(request.id.clone(), serde_json::to_value(result).unwrap())
        }

        "notifications/initialized" | "notifications/cancelled" => {
            success_response(request.id.clone(), serde_json::json!({}))
        }

        _ => error_response(
            request.id.clone(),
            error_codes::METHOD_NOT_FOUND,
            format!("unknown method: {}", request.method),
        ),
    }
}
