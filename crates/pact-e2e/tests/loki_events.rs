//! E2E test: Loki receives and queries pact journal events.
//!
//! Starts a Loki container, pushes journal event entries via the Loki
//! push API, and queries them back to verify the event streaming pipeline.

use pact_e2e::containers::loki::{Loki, LOKI_HTTP_PORT};
use reqwest::Client;
use serde_json::json;
use testcontainers::runners::AsyncRunner;

/// Loki accepts log pushes and returns them via query.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn loki_push_and_query_journal_events() {
    let container = Loki::default().start().await.expect("Loki container started");

    let host = container.get_host().await.expect("host");
    let port = container.get_host_port_ipv4(LOKI_HTTP_PORT).await.expect("port");
    let base_url = format!("http://{host}:{port}");
    let client = Client::new();

    // Verify Loki is ready
    let resp = client.get(format!("{base_url}/ready")).send().await.expect("ready check");
    assert_eq!(resp.status(), 200, "Loki should be ready");

    // Push journal events via Loki push API
    let now_ns = format!(
        "{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    );

    let push_body = json!({
        "streams": [
            {
                "stream": {
                    "component": "pact-journal",
                    "entry_type": "Commit",
                    "scope": "node-001",
                    "author": "admin@example.com"
                },
                "values": [
                    [&now_ns, r#"{"seq":1,"type":"Commit","scope":"node-001","author":"admin@example.com"}"#],
                ]
            },
            {
                "stream": {
                    "component": "pact-journal",
                    "entry_type": "DriftDetected",
                    "scope": "node-002",
                    "author": "system"
                },
                "values": [
                    [&now_ns, r#"{"seq":2,"type":"DriftDetected","scope":"node-002","author":"system"}"#],
                ]
            },
            {
                "stream": {
                    "component": "pact-journal",
                    "entry_type": "EmergencyStart",
                    "scope": "node-001",
                    "author": "admin@example.com"
                },
                "values": [
                    [&now_ns, r#"{"seq":3,"type":"EmergencyStart","scope":"node-001","reason":"maintenance"}"#],
                ]
            }
        ]
    });

    let resp = client
        .post(format!("{base_url}/loki/api/v1/push"))
        .header("Content-Type", "application/json")
        .json(&push_body)
        .send()
        .await
        .expect("push events");
    assert!(resp.status().is_success(), "Loki push should succeed: {}", resp.status());

    // Wait for ingestion
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // Query all pact-journal events
    let resp = client
        .get(format!("{base_url}/loki/api/v1/query_range"))
        .query(&[
            ("query", r#"{component="pact-journal"}"#),
            ("limit", "100"),
            (
                "start",
                &format!(
                    "{}",
                    chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0) - 60_000_000_000
                ),
            ),
            (
                "end",
                &format!(
                    "{}",
                    chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0) + 60_000_000_000
                ),
            ),
        ])
        .send()
        .await
        .expect("query events");
    assert!(resp.status().is_success(), "query should succeed");

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "success", "query should return success");

    let result = &body["data"]["result"];
    assert!(result.is_array(), "should have results");
    // We pushed 3 streams, expect at least some results
    let stream_count = result.as_array().map_or(0, Vec::len);
    assert!(stream_count >= 1, "should have at least 1 stream, got {stream_count}");

    // Query specifically for DriftDetected events
    let resp = client
        .get(format!("{base_url}/loki/api/v1/query_range"))
        .query(&[
            ("query", r#"{component="pact-journal", entry_type="DriftDetected"}"#),
            ("limit", "100"),
            (
                "start",
                &format!(
                    "{}",
                    chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0) - 60_000_000_000
                ),
            ),
            (
                "end",
                &format!(
                    "{}",
                    chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0) + 60_000_000_000
                ),
            ),
        ])
        .send()
        .await
        .expect("query drift events");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "success");
}

/// Loki label queries return pact event types.
#[tokio::test]
async fn loki_label_values() {
    let container = Loki::default().start().await.expect("Loki container started");

    let host = container.get_host().await.expect("host");
    let port = container.get_host_port_ipv4(LOKI_HTTP_PORT).await.expect("port");
    let base_url = format!("http://{host}:{port}");
    let client = Client::new();

    // Push an event first
    let now_ns = format!(
        "{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    );
    client
        .post(format!("{base_url}/loki/api/v1/push"))
        .header("Content-Type", "application/json")
        .json(&json!({
            "streams": [{
                "stream": {
                    "component": "pact-journal",
                    "entry_type": "Commit"
                },
                "values": [[&now_ns, "test commit event"]]
            }]
        }))
        .send()
        .await
        .expect("push");

    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // Query available labels
    let resp =
        client.get(format!("{base_url}/loki/api/v1/labels")).send().await.expect("labels query");
    assert!(resp.status().is_success());

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "success");
    let labels = body["data"].as_array().expect("labels array");
    assert!(
        labels.iter().any(|l| l.as_str() == Some("component")),
        "should have 'component' label"
    );
}
