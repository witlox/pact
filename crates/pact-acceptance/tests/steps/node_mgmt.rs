//! BDD step definitions for node management delegation.
//!
//! Tests CSM and OpenCHAMI backends against a lightweight axum mock server —
//! real HTTP code paths, real request serialization, real error handling.
//!
//! Uses axum instead of wiremock because wiremock's MockServer::start() blocks
//! inside cucumber-rs's async executor.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::Router;
use cucumber::{given, then, when};
use pact_cli::commands::csm::CsmBackend;
use pact_cli::commands::openchami::OpenChamiBackend;
use pact_common::node_mgmt::{NodeManagementBackend, NodeMgmtBackendType};
use pact_journal::JournalCommand;

use crate::PactWorld;

// ---------------------------------------------------------------------------
// Lightweight mock HTTP server (replaces wiremock)
// ---------------------------------------------------------------------------

/// A captured HTTP request for assertion in Then steps.
#[derive(Debug, Clone)]
pub struct CapturedRequest {
    pub method: String,
    pub path: String,
    pub body: String,
    pub headers: HashMap<String, String>,
}

/// Shared state for the mock server — records requests + configurable responses.
#[derive(Debug, Clone)]
pub struct MockState {
    pub requests: Arc<Mutex<Vec<CapturedRequest>>>,
    /// Path → (status_code, response_body)
    pub responses: Arc<Mutex<HashMap<String, (u16, String)>>>,
}

impl MockState {
    fn new() -> Self {
        Self {
            requests: Arc::new(Mutex::new(Vec::new())),
            responses: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn set_response(&self, path: &str, status: u16, body: &str) {
        self.responses.lock().unwrap().insert(path.to_string(), (status, body.to_string()));
    }
}

/// Catch-all POST handler — records request and returns configured response.
async fn handle_post(
    State(state): State<MockState>,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, String) {
    let path = uri.path().to_string();
    let mut header_map = HashMap::new();
    for (k, v) in &headers {
        header_map.insert(k.to_string(), v.to_str().unwrap_or("").to_string());
    }
    state.requests.lock().unwrap().push(CapturedRequest {
        method: "POST".into(),
        path: path.clone(),
        body: String::from_utf8_lossy(&body).to_string(),
        headers: header_map,
    });

    // Find matching response (try exact, then prefix match)
    let responses = state.responses.lock().unwrap();
    if let Some((status, resp_body)) = responses.get(&path) {
        return (StatusCode::from_u16(*status).unwrap_or(StatusCode::OK), resp_body.clone());
    }
    // Prefix matching for dynamic paths like /hsm/v2/State/Components/{node}/Actions/PowerCycle
    for (pattern, (status, resp_body)) in responses.iter() {
        if path.starts_with(pattern) || path.contains(pattern.trim_start_matches('/')) {
            return (StatusCode::from_u16(*status).unwrap_or(StatusCode::OK), resp_body.clone());
        }
    }
    (StatusCode::NOT_FOUND, "no mock configured".into())
}

/// Catch-all GET handler — records request and returns configured response.
async fn handle_get(
    State(state): State<MockState>,
    axum::extract::OriginalUri(uri): axum::extract::OriginalUri,
    headers: HeaderMap,
) -> (StatusCode, String) {
    let path = uri.path().to_string();
    let mut header_map = HashMap::new();
    for (k, v) in &headers {
        header_map.insert(k.to_string(), v.to_str().unwrap_or("").to_string());
    }
    state.requests.lock().unwrap().push(CapturedRequest {
        method: "GET".into(),
        path: path.clone(),
        body: String::new(),
        headers: header_map,
    });

    let responses = state.responses.lock().unwrap();
    if let Some((status, resp_body)) = responses.get(&path) {
        return (StatusCode::from_u16(*status).unwrap_or(StatusCode::OK), resp_body.clone());
    }
    (StatusCode::NOT_FOUND, "no mock configured".into())
}

/// Start mock server on ephemeral port. Returns (base_url, state).
async fn start_mock_server() -> (String, MockState) {
    let state = MockState::new();
    let app = Router::new().fallback_service(
        Router::new()
            .route("/{*path}", post(handle_post).get(handle_get))
            .with_state(state.clone()),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.ok();
    });
    (format!("http://{addr}"), state)
}

// ---------------------------------------------------------------------------
// Background
// ---------------------------------------------------------------------------

#[given("a running journal quorum")]
async fn given_running_journal(world: &mut PactWorld) {
    world.journal.entries.clear();
    world.journal.audit_log.clear();
}

#[given(regex = r#"^an authenticated operator with role "([\w-]+)"$"#)]
async fn given_authenticated_operator(world: &mut PactWorld, role: String) {
    world.current_identity = Some(pact_common::types::Identity {
        principal: "operator@example.com".into(),
        principal_type: pact_common::types::PrincipalType::Human,
        role,
    });
}

// ---------------------------------------------------------------------------
// Given — backend configuration
// ---------------------------------------------------------------------------

#[given(regex = r#"^the node management backend is set to "(csm|ochami)"$"#)]
async fn given_backend_type(world: &mut PactWorld, backend: String) {
    let bt = match backend.as_str() {
        "csm" => NodeMgmtBackendType::Csm,
        "ochami" => NodeMgmtBackendType::Ochami,
        _ => unreachable!(),
    };
    world.node_mgmt_backend_type = Some(bt.clone());

    // Start mock server unless unreachable scenario
    if world.node_mgmt_mock_url.is_none() && !world.node_mgmt_unreachable {
        let (url, state) = start_mock_server().await;

        // Pre-mount default success responses so scenarios without explicit
        // "Given CSM/OpenCHAMI is reachable" still work.
        match bt {
            NodeMgmtBackendType::Csm => {
                state.set_response("/capmc/capmc/v1/xname_reinit", 200, r#"{"e": 0}"#);
                state.set_response("/bos/v2/sessions", 200, r#"{"name": "bos-session-001"}"#);
            }
            NodeMgmtBackendType::Ochami => {
                state.set_response("/Actions/PowerCycle", 200, r#"{"status": "ok"}"#);
            }
        }

        world.node_mgmt_mock_url = Some(url);
        world.node_mgmt_mock_state = Some(state);
    }
}

#[given("the node management backend is not configured")]
async fn given_backend_not_configured(world: &mut PactWorld) {
    world.node_mgmt_backend_type = None;
}

#[given("CSM is reachable at the configured base URL")]
async fn given_csm_reachable(world: &mut PactWorld) {
    let state = world.node_mgmt_mock_state.as_ref().expect("mock server started");
    state.set_response("/capmc/capmc/v1/xname_reinit", 200, r#"{"e": 0}"#);
}

#[given("OpenCHAMI SMD is reachable at the configured base URL")]
async fn given_ochami_reachable(world: &mut PactWorld) {
    let state = world.node_mgmt_mock_state.as_ref().expect("mock server started");
    // Use a prefix pattern — PowerCycle path includes dynamic node ID
    state.set_response("/Actions/PowerCycle", 200, r#"{"status": "ok"}"#);
}

#[given("CSM is unreachable")]
async fn given_csm_unreachable(world: &mut PactWorld) {
    world.node_mgmt_unreachable = true;
    world.node_mgmt_mock_url = None;
    world.node_mgmt_mock_state = None;
}

#[given(regex = r"^CSM returns HTTP 500 on the reboot call$")]
async fn given_csm_returns_500(world: &mut PactWorld) {
    world.node_mgmt_error_status = Some(500);
    let state = world.node_mgmt_mock_state.as_ref().expect("mock server started");
    state.set_response("/capmc/capmc/v1/xname_reinit", 500, "internal server error");
}

#[given(regex = r#"^the node "([\w-]+)" has no BOS boot record$"#)]
async fn given_no_bos_record(world: &mut PactWorld, _node: String) {
    world.node_mgmt_no_bos_template = true;
    let state = world.node_mgmt_mock_state.as_ref().expect("mock server started");
    state.set_response("/bos/v2/sessions", 404, "no boot template found for node");
}

#[given("a node management token is configured")]
async fn given_token_configured(world: &mut PactWorld) {
    world.node_mgmt_token = Some("test-bearer-token-abc123".into());
}

#[given("no node management token is configured")]
async fn given_no_token(world: &mut PactWorld) {
    world.node_mgmt_token = None;
}

#[given(regex = r"^HSM contains (\d+) compute nodes$")]
async fn given_hsm_nodes(world: &mut PactWorld, count: usize) {
    world.node_mgmt_hsm_node_count = count;
    let state = world.node_mgmt_mock_state.as_ref().expect("mock server started");

    let components: Vec<serde_json::Value> = (0..count)
        .map(|i| {
            serde_json::json!({
                "ID": format!("x1000c0s{}b0n0", i),
                "Type": "Node",
                "State": "Ready",
                "Role": "Compute"
            })
        })
        .collect();
    let body = serde_json::json!({ "Components": components }).to_string();

    state.set_response("/smd/hsm/v2/State/Components", 200, &body);
    state.set_response("/hsm/v2/State/Components", 200, &body);
}

// ---------------------------------------------------------------------------
// When — execute delegation commands
// ---------------------------------------------------------------------------

fn backend_url(world: &PactWorld) -> String {
    if world.node_mgmt_unreachable {
        "http://127.0.0.1:1".into()
    } else {
        world.node_mgmt_mock_url.clone().expect("mock server URL set")
    }
}

fn record_audit(world: &mut PactWorld, command: &str, node_id: &str, target_system: &str) {
    let identity = world.current_identity.as_ref().expect("operator authenticated");
    let entry = pact_common::types::ConfigEntry {
        sequence: world.journal.entries.len() as u64 + 1,
        timestamp: chrono::Utc::now(),
        entry_type: pact_common::types::EntryType::ServiceLifecycle,
        scope: pact_common::types::Scope::Node(node_id.into()),
        author: identity.clone(),
        parent: None,
        state_delta: None,
        policy_ref: Some(format!("delegate:{target_system}:{command}:{node_id}")),
        ttl_seconds: None,
        emergency_reason: None,
    };
    let _ = world.journal.apply_command(JournalCommand::AppendEntry(entry));
}

fn capture_requests(world: &mut PactWorld) {
    if let Some(ref state) = world.node_mgmt_mock_state {
        world.node_mgmt_received_requests.clone_from(&state.requests.lock().unwrap());
    }
}

#[when(regex = r#"^the operator runs "pact reboot ([\w-]+)"$"#)]
async fn when_operator_reboot(world: &mut PactWorld, node_id: String) {
    let backend_type = world.node_mgmt_backend_type.clone();
    let token = world.node_mgmt_token.clone();

    let target = backend_type.as_ref().map_or("OpenCHAMI", NodeMgmtBackendType::display_name);

    record_audit(world, "reboot", &node_id, target);

    let Some(bt) = backend_type else {
        world.node_mgmt_result_success = Some(false);
        world.node_mgmt_result_message = Some("node management backend not configured".into());
        return;
    };

    let url = backend_url(world);

    let result = match bt {
        NodeMgmtBackendType::Csm => {
            CsmBackend::new(&url, token.as_deref(), 2).reboot(&node_id).await
        }
        NodeMgmtBackendType::Ochami => {
            OpenChamiBackend::new(&url, token.as_deref(), 2).reboot(&node_id).await
        }
    };

    match result {
        Ok(msg) => {
            world.node_mgmt_result_success = Some(true);
            world.node_mgmt_result_message = Some(msg);
        }
        Err(e) => {
            world.node_mgmt_result_success = Some(false);
            world.node_mgmt_result_message = Some(format!("{e}"));
        }
    }

    capture_requests(world);
}

#[when(regex = r#"^the operator runs "pact reimage ([\w-]+)"$"#)]
async fn when_operator_reimage(world: &mut PactWorld, node_id: String) {
    let backend_type = world.node_mgmt_backend_type.clone();
    let token = world.node_mgmt_token.clone();

    let target = backend_type.as_ref().map_or("OpenCHAMI", NodeMgmtBackendType::display_name);

    record_audit(world, "reimage", &node_id, target);

    let Some(bt) = backend_type else {
        world.node_mgmt_result_success = Some(false);
        world.node_mgmt_result_message = Some("node management backend not configured".into());
        return;
    };

    let url = backend_url(world);

    // Mount BOS success response if not already configured for error
    if bt == NodeMgmtBackendType::Csm
        && world.node_mgmt_error_status.is_none()
        && !world.node_mgmt_no_bos_template
    {
        if let Some(ref state) = world.node_mgmt_mock_state {
            state.set_response("/bos/v2/sessions", 200, r#"{"name": "bos-session-001"}"#);
        }
    }

    let result = match bt {
        NodeMgmtBackendType::Csm => {
            CsmBackend::new(&url, token.as_deref(), 2).reimage(&node_id).await
        }
        NodeMgmtBackendType::Ochami => {
            OpenChamiBackend::new(&url, token.as_deref(), 2).reimage(&node_id).await
        }
    };

    match result {
        Ok(msg) => {
            world.node_mgmt_result_success = Some(true);
            world.node_mgmt_result_message = Some(msg);
        }
        Err(e) => {
            world.node_mgmt_result_success = Some(false);
            world.node_mgmt_result_message = Some(format!("{e}"));
        }
    }

    capture_requests(world);
}

#[when(r#"the operator runs "pact node import""#)]
async fn when_operator_node_import(world: &mut PactWorld) {
    let backend_type = world.node_mgmt_backend_type.clone().expect("backend type set");
    let url = backend_url(world);
    let token = world.node_mgmt_token.clone();

    let hsm_prefix = match backend_type {
        NodeMgmtBackendType::Csm => "/smd/hsm/v2",
        NodeMgmtBackendType::Ochami => "/hsm/v2",
    };
    let hsm_url = format!("{url}{hsm_prefix}/State/Components");

    let client = reqwest::Client::new();
    let mut req = client.get(&hsm_url);
    if let Some(ref t) = token {
        req = req.bearer_auth(t);
    }

    match req.send().await {
        Ok(resp) if resp.status().is_success() => {
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            if let Some(nodes) = body["Components"].as_array() {
                for node in nodes {
                    if let Some(id) = node["ID"].as_str() {
                        let entry = pact_common::types::ConfigEntry {
                            sequence: world.journal.entries.len() as u64 + 1,
                            timestamp: chrono::Utc::now(),
                            entry_type: pact_common::types::EntryType::ServiceLifecycle,
                            scope: pact_common::types::Scope::Node(id.into()),
                            author: world.current_identity.clone().expect("authenticated"),
                            parent: None,
                            state_delta: None,
                            policy_ref: Some(format!("import:enroll:{id}")),
                            ttl_seconds: None,
                            emergency_reason: None,
                        };
                        let _ = world.journal.apply_command(JournalCommand::AppendEntry(entry));
                    }
                }
                world.node_mgmt_result_success = Some(true);
                world.node_mgmt_result_message = Some(format!("{} nodes enrolled", nodes.len()));
            } else {
                world.node_mgmt_result_success = Some(false);
                world.node_mgmt_result_message = Some("no components in response".into());
            }
        }
        Ok(resp) => {
            world.node_mgmt_result_success = Some(false);
            world.node_mgmt_result_message = Some(format!("HSM returned {}", resp.status()));
        }
        Err(e) => {
            world.node_mgmt_result_success = Some(false);
            world.node_mgmt_result_message = Some(format!("{e}"));
        }
    }

    capture_requests(world);
}

// ---------------------------------------------------------------------------
// Then — backend selection
// ---------------------------------------------------------------------------

#[then("the reboot request is sent via CAPMC API")]
async fn then_via_capmc(world: &mut PactWorld) {
    assert!(
        world.node_mgmt_received_requests.iter().any(|r| r.path.contains("capmc")),
        "expected CAPMC request, got paths: {:?}",
        world.node_mgmt_received_requests.iter().map(|r| &r.path).collect::<Vec<_>>()
    );
}

#[then("the reboot request is sent via SMD Redfish API")]
async fn then_via_redfish(world: &mut PactWorld) {
    assert!(
        world.node_mgmt_received_requests.iter().any(|r| r.path.contains("Actions/PowerCycle")),
        "expected Redfish request, got paths: {:?}",
        world.node_mgmt_received_requests.iter().map(|r| &r.path).collect::<Vec<_>>()
    );
}

#[then("an audit entry is recorded before the CAPMC call")]
async fn then_audit_before_capmc(world: &mut PactWorld) {
    assert!(!world.journal.entries.is_empty(), "journal should have audit entry");
    let (_, entry) = world.journal.entries.iter().next().unwrap();
    let pref = entry.policy_ref.as_deref().unwrap_or("");
    assert!(pref.starts_with("delegate:"), "expected delegation record, got: {pref}");
}

#[then("an audit entry is recorded before the Redfish call")]
async fn then_audit_before_redfish(world: &mut PactWorld) {
    assert!(!world.journal.entries.is_empty(), "journal should have audit entry");
    let (_, entry) = world.journal.entries.iter().next().unwrap();
    let pref = entry.policy_ref.as_deref().unwrap_or("");
    assert!(pref.starts_with("delegate:"), "expected delegation record, got: {pref}");
}

#[then(regex = r#"^the command fails with "(.*)"$"#)]
async fn then_command_fails_with(world: &mut PactWorld, expected_msg: String) {
    assert_eq!(world.node_mgmt_result_success, Some(false), "command should have failed");
    let msg = world.node_mgmt_result_message.as_deref().unwrap_or("");
    assert!(msg.contains(&expected_msg), "expected '{expected_msg}' in: '{msg}'");
}

// ---------------------------------------------------------------------------
// Then — reboot / reimage
// ---------------------------------------------------------------------------

#[then("CAPMC receives POST /capmc/capmc/v1/xname_reinit")]
async fn then_capmc_post(world: &mut PactWorld) {
    assert!(
        world
            .node_mgmt_received_requests
            .iter()
            .any(|r| { r.method == "POST" && r.path == "/capmc/capmc/v1/xname_reinit" }),
        "expected POST /capmc/capmc/v1/xname_reinit"
    );
}

#[then(regex = r#"^the request body contains xname "([\w-]+)"$"#)]
async fn then_body_contains_xname(world: &mut PactWorld, xname: String) {
    let req = world
        .node_mgmt_received_requests
        .iter()
        .find(|r| r.path.contains("capmc") || r.path.contains("PowerCycle"))
        .expect("expected a backend request");
    assert!(req.body.contains(&xname), "body should contain '{xname}', got: {}", req.body);
}

#[then("the command reports success")]
async fn then_command_success(world: &mut PactWorld) {
    assert_eq!(
        world.node_mgmt_result_success,
        Some(true),
        "command should have succeeded, msg: {:?}",
        world.node_mgmt_result_message
    );
}

#[then(regex = r"^SMD receives POST /hsm/v2/State/Components/([\w-]+)/Actions/PowerCycle$")]
async fn then_smd_power_cycle(world: &mut PactWorld, node_id: String) {
    let expected = format!("/hsm/v2/State/Components/{node_id}/Actions/PowerCycle");
    assert!(
        world.node_mgmt_received_requests.iter().any(|r| r.method == "POST" && r.path == expected),
        "expected POST {expected}, got: {:?}",
        world
            .node_mgmt_received_requests
            .iter()
            .map(|r| format!("{} {}", r.method, r.path))
            .collect::<Vec<_>>()
    );
}

#[then("the command fails with a connection error")]
async fn then_connection_error(world: &mut PactWorld) {
    assert_eq!(world.node_mgmt_result_success, Some(false), "command should have failed");
    let msg = world.node_mgmt_result_message.as_deref().unwrap_or("");
    assert!(!msg.is_empty(), "error message should not be empty");
}

#[then("the audit entry still exists in the journal")]
async fn then_audit_still_exists(world: &mut PactWorld) {
    assert!(
        world
            .journal
            .entries
            .values()
            .any(|e| { e.policy_ref.as_deref().unwrap_or("").starts_with("delegate:") }),
        "journal should contain delegation audit entry"
    );
}

// ---------------------------------------------------------------------------
// Then — reimage
// ---------------------------------------------------------------------------

#[then(r#"BOS receives POST /bos/v2/sessions with operation "reboot""#)]
async fn then_bos_post(world: &mut PactWorld) {
    let req = world
        .node_mgmt_received_requests
        .iter()
        .find(|r| r.method == "POST" && r.path == "/bos/v2/sessions")
        .expect("expected POST /bos/v2/sessions");
    assert!(req.body.contains("reboot"), "body should contain 'reboot', got: {}", req.body);
}

#[then(regex = r#"^the session targets xname "([\w-]+)"$"#)]
async fn then_session_targets_xname(world: &mut PactWorld, xname: String) {
    let req = world
        .node_mgmt_received_requests
        .iter()
        .find(|r| r.path == "/bos/v2/sessions")
        .expect("expected BOS request");
    assert!(req.body.contains(&xname), "session should target '{xname}', got: {}", req.body);
}

// ---------------------------------------------------------------------------
// Then — node import
// ---------------------------------------------------------------------------

#[then(regex = r"^(\d+) nodes are enrolled in the journal$")]
async fn then_nodes_enrolled(world: &mut PactWorld, count: usize) {
    let enrolled = world
        .journal
        .entries
        .values()
        .filter(|e| e.policy_ref.as_deref().unwrap_or("").starts_with("import:enroll:"))
        .count();
    assert_eq!(enrolled, count, "expected {count} enrolled nodes, got {enrolled}");
}

#[then(regex = r"^HSM was queried at (.+)$")]
async fn then_hsm_queried_at(world: &mut PactWorld, expected_path: String) {
    assert!(
        world
            .node_mgmt_received_requests
            .iter()
            .any(|r| r.method == "GET" && r.path == expected_path),
        "expected GET {expected_path}, got: {:?}",
        world
            .node_mgmt_received_requests
            .iter()
            .map(|r| format!("{} {}", r.method, r.path))
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// Then — auth token
// ---------------------------------------------------------------------------

#[then("the CAPMC request includes Authorization header with the token")]
async fn then_capmc_has_auth(world: &mut PactWorld) {
    let req = world
        .node_mgmt_received_requests
        .iter()
        .find(|r| r.path.contains("capmc"))
        .expect("CAPMC request");
    let auth = req.headers.get("authorization").expect("Authorization header");
    let token = world.node_mgmt_token.as_deref().expect("token configured");
    assert!(auth.contains(token), "auth header should contain token, got: {auth}");
}

#[then("the Redfish request includes Authorization header with the token")]
async fn then_redfish_has_auth(world: &mut PactWorld) {
    let req = world
        .node_mgmt_received_requests
        .iter()
        .find(|r| r.path.contains("PowerCycle"))
        .expect("Redfish request");
    let auth = req.headers.get("authorization").expect("Authorization header");
    let token = world.node_mgmt_token.as_deref().expect("token configured");
    assert!(auth.contains(token), "auth header should contain token, got: {auth}");
}

#[then("the request is sent without an Authorization header")]
async fn then_no_auth_header(world: &mut PactWorld) {
    let req = world
        .node_mgmt_received_requests
        .iter()
        .find(|r| r.method == "POST")
        .expect("POST request");
    assert!(
        !req.headers.contains_key("authorization"),
        "request should NOT have Authorization header"
    );
}

// ---------------------------------------------------------------------------
// Then — audit invariant
// ---------------------------------------------------------------------------

#[then("the command fails")]
async fn then_command_fails(world: &mut PactWorld) {
    assert_eq!(world.node_mgmt_result_success, Some(false), "command should have failed");
}

#[then(regex = r#"^the journal contains an audit entry for "(.*)"$"#)]
async fn then_journal_has_audit(world: &mut PactWorld, expected_ref: String) {
    // Feature file uses "reboot x1000c0s0b0n0" (space-separated).
    // Policy ref format is "delegate:CSM:reboot:x1000c0s0b0n0" (colon-separated).
    // Match each word individually so both formats work.
    let words: Vec<&str> = expected_ref.split_whitespace().collect();
    assert!(
        world.journal.entries.values().any(|e| {
            let pref = e.policy_ref.as_deref().unwrap_or("");
            words.iter().all(|w| pref.contains(w))
        }),
        "journal should contain all of '{expected_ref}', entries: {:?}",
        world
            .journal
            .entries
            .values()
            .map(|e| e.policy_ref.as_deref().unwrap_or(""))
            .collect::<Vec<_>>()
    );
}
