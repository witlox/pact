//! E2E test: OPA policy evaluation via real OPA container.
//!
//! Verifies that pact authorization policies can be pushed to OPA
//! and evaluated through its REST API, matching the ADR-003 design
//! (OPA/Rego on journal nodes, called via localhost REST).

use reqwest::Client;
use serde_json::json;
use testcontainers::runners::AsyncRunner;
use testcontainers::ImageExt;

use pact_e2e::containers::opa::{Opa, OPA_PORT};

/// Push a Rego policy to OPA and evaluate it.
#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn opa_evaluates_pact_authorization_policy() {
    let container = Opa::default()
        .with_cmd(["run", "--server", "--addr", "0.0.0.0:8181", "--log-level", "error"])
        .start()
        .await
        .expect("OPA container started");

    let host = container.get_host().await.expect("host");
    let port = container.get_host_port_ipv4(OPA_PORT).await.expect("port");
    let base_url = format!("http://{host}:{port}");
    let client = Client::new();

    // Push a pact authorization policy
    let policy = r#"
        package pact.authz

        import rego.v1

        default allow := false

        # Platform admins can do anything
        allow if {
            input.identity.role == "pact-platform-admin"
        }

        # Ops roles can commit/rollback/exec on their vCluster
        allow if {
            input.identity.role == concat("-", ["pact-ops", input.scope.vcluster])
            input.action in ["commit", "rollback", "exec", "status", "diff", "log"]
        }

        # Viewer roles get read-only access
        allow if {
            input.identity.role == concat("-", ["pact-viewer", input.scope.vcluster])
            input.action in ["status", "diff", "log"]
        }

        # Viewers cannot commit
        deny_reason := "viewers cannot modify state" if {
            contains(input.identity.role, "viewer")
            input.action in ["commit", "rollback", "exec", "emergency"]
        }
    "#;

    let resp = client
        .put(format!("{base_url}/v1/policies/pact_authz"))
        .header("Content-Type", "text/plain")
        .body(policy)
        .send()
        .await
        .expect("push policy");
    assert!(resp.status().is_success(), "failed to push policy: {}", resp.status());

    // Evaluate: platform admin should be allowed
    let resp = client
        .post(format!("{base_url}/v1/data/pact/authz/allow"))
        .json(&json!({
            "input": {
                "identity": {
                    "principal": "admin@example.com",
                    "role": "pact-platform-admin"
                },
                "action": "commit",
                "scope": { "vcluster": "ml-training" }
            }
        }))
        .send()
        .await
        .expect("evaluate admin");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["result"], json!(true), "platform admin should be allowed");

    // Evaluate: ops role can commit on their vCluster
    let resp = client
        .post(format!("{base_url}/v1/data/pact/authz/allow"))
        .json(&json!({
            "input": {
                "identity": {
                    "principal": "ops@example.com",
                    "role": "pact-ops-ml-training"
                },
                "action": "commit",
                "scope": { "vcluster": "ml-training" }
            }
        }))
        .send()
        .await
        .expect("evaluate ops");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["result"], json!(true), "ops should be allowed to commit");

    // Evaluate: viewer cannot commit
    let resp = client
        .post(format!("{base_url}/v1/data/pact/authz/allow"))
        .json(&json!({
            "input": {
                "identity": {
                    "principal": "viewer@example.com",
                    "role": "pact-viewer-ml-training"
                },
                "action": "commit",
                "scope": { "vcluster": "ml-training" }
            }
        }))
        .send()
        .await
        .expect("evaluate viewer");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["result"], json!(false), "viewer should be denied commit");

    // Evaluate: viewer can read status
    let resp = client
        .post(format!("{base_url}/v1/data/pact/authz/allow"))
        .json(&json!({
            "input": {
                "identity": {
                    "principal": "viewer@example.com",
                    "role": "pact-viewer-ml-training"
                },
                "action": "status",
                "scope": { "vcluster": "ml-training" }
            }
        }))
        .send()
        .await
        .expect("evaluate viewer status");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["result"], json!(true), "viewer should see status");

    // Evaluate: ops on wrong vCluster should be denied
    let resp = client
        .post(format!("{base_url}/v1/data/pact/authz/allow"))
        .json(&json!({
            "input": {
                "identity": {
                    "principal": "ops@example.com",
                    "role": "pact-ops-ml-training"
                },
                "action": "commit",
                "scope": { "vcluster": "storage-ops" }
            }
        }))
        .send()
        .await
        .expect("evaluate cross-vcluster");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["result"], json!(false), "ops should not cross vCluster boundary");

    // Verify deny_reason for viewer commit attempt
    let resp = client
        .post(format!("{base_url}/v1/data/pact/authz/deny_reason"))
        .json(&json!({
            "input": {
                "identity": {
                    "principal": "viewer@example.com",
                    "role": "pact-viewer-ml-training"
                },
                "action": "commit",
                "scope": { "vcluster": "ml-training" }
            }
        }))
        .send()
        .await
        .expect("evaluate deny reason");
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["result"], "viewers cannot modify state", "should get denial reason");
}

/// OPA health endpoint returns 200.
#[tokio::test]
async fn opa_health_check() {
    let container = Opa::default()
        .with_cmd(["run", "--server", "--addr", "0.0.0.0:8181", "--log-level", "error"])
        .start()
        .await
        .expect("OPA container started");

    let host = container.get_host().await.expect("host");
    let port = container.get_host_port_ipv4(OPA_PORT).await.expect("port");

    let resp = Client::new()
        .get(format!("http://{host}:{port}/health"))
        .send()
        .await
        .expect("health check");
    assert_eq!(resp.status(), 200);
}
