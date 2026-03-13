//! MCP protocol types — JSON-RPC messages and tool definitions.
//!
//! MCP uses JSON-RPC 2.0 over stdio. The server advertises tools on
//! `initialize`, then handles `tools/call` requests.

use serde::{Deserialize, Serialize};

/// JSON-RPC 2.0 request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// JSON-RPC 2.0 response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// MCP tool definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: serde_json::Value,
}

/// MCP tool call request parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallParams {
    pub name: String,
    #[serde(default)]
    pub arguments: serde_json::Value,
}

/// MCP tool call result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallResult {
    pub content: Vec<ContentBlock>,
    #[serde(rename = "isError", default)]
    pub is_error: bool,
}

/// Content block in a tool result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

/// MCP server info returned on initialize.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

/// MCP capabilities returned on initialize.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerCapabilities {
    pub tools: ToolsCapability,
}

/// Tools capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsCapability {
    #[serde(rename = "listChanged", default)]
    pub list_changed: bool,
}

/// Build a successful JSON-RPC response.
pub fn success_response(id: serde_json::Value, result: serde_json::Value) -> JsonRpcResponse {
    JsonRpcResponse { jsonrpc: "2.0".into(), id, result: Some(result), error: None }
}

/// Build an error JSON-RPC response.
pub fn error_response(id: serde_json::Value, code: i32, message: String) -> JsonRpcResponse {
    JsonRpcResponse {
        jsonrpc: "2.0".into(),
        id,
        result: None,
        error: Some(JsonRpcError { code, message, data: None }),
    }
}

/// Build a text content block.
pub fn text_content(text: impl Into<String>) -> ContentBlock {
    ContentBlock { content_type: "text".into(), text: text.into() }
}

/// Build a tool call result.
pub fn tool_result(text: impl Into<String>, is_error: bool) -> ToolCallResult {
    ToolCallResult { content: vec![text_content(text)], is_error }
}

// JSON-RPC error codes.
pub mod error_codes {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_rpc_request_deserialize() {
        let json = r#"{
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {"name": "pact_status", "arguments": {"node": "node042"}}
        }"#;
        let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.method, "tools/call");
        assert_eq!(req.id, 1);
    }

    #[test]
    fn json_rpc_response_serialize() {
        let resp = success_response(serde_json::json!(1), serde_json::json!({"ok": true}));
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"ok\":true"));
        assert!(!json.contains("error"));
    }

    #[test]
    fn json_rpc_error_response_serialize() {
        let resp = error_response(
            serde_json::json!(2),
            error_codes::METHOD_NOT_FOUND,
            "unknown method".into(),
        );
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("-32601"));
        assert!(json.contains("unknown method"));
        assert!(!json.contains("\"result\""));
    }

    #[test]
    fn tool_definition_serialize() {
        let tool = ToolDefinition {
            name: "pact_status".into(),
            description: "Query node status".into(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "node": {"type": "string"}
                }
            }),
        };
        let json = serde_json::to_string(&tool).unwrap();
        assert!(json.contains("pact_status"));
        assert!(json.contains("inputSchema"));
    }

    #[test]
    fn tool_call_params_deserialize() {
        let json =
            r#"{"name": "pact_exec", "arguments": {"node": "node042", "command": "hostname"}}"#;
        let params: ToolCallParams = serde_json::from_str(json).unwrap();
        assert_eq!(params.name, "pact_exec");
        assert_eq!(params.arguments["node"], "node042");
    }

    #[test]
    fn tool_result_success() {
        let result = tool_result("Node: node042 State: COMMITTED", false);
        assert!(!result.is_error);
        assert_eq!(result.content.len(), 1);
        assert_eq!(result.content[0].content_type, "text");
    }

    #[test]
    fn tool_result_error() {
        let result = tool_result("Permission denied", true);
        assert!(result.is_error);
    }

    #[test]
    fn text_content_creates_text_block() {
        let block = text_content("hello");
        assert_eq!(block.content_type, "text");
        assert_eq!(block.text, "hello");
    }

    #[test]
    fn server_capabilities_serialize() {
        let caps = ServerCapabilities { tools: ToolsCapability { list_changed: false } };
        let json = serde_json::to_string(&caps).unwrap();
        assert!(json.contains("tools"));
        assert!(json.contains("listChanged"));
    }
}
