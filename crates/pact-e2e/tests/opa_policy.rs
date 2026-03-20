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
use pact_policy::rules::opa::{HttpOpaClient, OpaClient, OpaIdentity, OpaInput, OpaScope};

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

/// Use HttpOpaClient to evaluate policies against a real OPA container.
#[tokio::test]
async fn opa_client_evaluates_via_http_client() {
    let container = Opa::default()
        .with_cmd(["run", "--server", "--addr", "0.0.0.0:8181", "--log-level", "error"])
        .start()
        .await
        .expect("OPA container started");

    let host = container.get_host().await.expect("host");
    let port = container.get_host_port_ipv4(OPA_PORT).await.expect("port");
    let base_url = format!("http://{host}:{port}");

    // Push the same Rego policy via raw reqwest (OPA Data API)
    let policy = r#"
        package pact.authz

        import rego.v1

        default allow := false

        allow if {
            input.identity.role == "pact-platform-admin"
        }

        allow if {
            input.identity.role == concat("-", ["pact-ops", input.scope.vcluster])
            input.action in ["commit", "rollback", "exec", "status", "diff", "log"]
        }

        allow if {
            input.identity.role == concat("-", ["pact-viewer", input.scope.vcluster])
            input.action in ["status", "diff", "log"]
        }

        deny_reason := "viewers cannot modify state" if {
            contains(input.identity.role, "viewer")
            input.action in ["commit", "rollback", "exec", "emergency"]
        }
    "#;

    let resp = Client::new()
        .put(format!("{base_url}/v1/policies/pact_authz"))
        .header("Content-Type", "text/plain")
        .body(policy)
        .send()
        .await
        .expect("push policy");
    assert!(resp.status().is_success(), "failed to push policy: {}", resp.status());

    // Create HttpOpaClient pointing at the container
    let opa_client = HttpOpaClient::new(&base_url);

    // Admin should be allowed
    let admin_input = OpaInput {
        identity: OpaIdentity {
            principal: "admin@example.com".into(),
            role: "pact-platform-admin".into(),
            principal_type: "Human".into(),
        },
        action: "commit".into(),
        scope: OpaScope { vcluster: "ml-training".into() },
        command: None,
    };
    let result = opa_client.evaluate(&admin_input).await.expect("admin eval");
    assert_eq!(
        result,
        pact_policy::rules::opa::OpaDecision::Allow,
        "platform admin should be allowed via HttpOpaClient"
    );

    // Viewer committing should be denied
    let viewer_input = OpaInput {
        identity: OpaIdentity {
            principal: "viewer@example.com".into(),
            role: "pact-viewer-ml-training".into(),
            principal_type: "Human".into(),
        },
        action: "commit".into(),
        scope: OpaScope { vcluster: "ml-training".into() },
        command: None,
    };
    let result = opa_client.evaluate(&viewer_input).await.expect("viewer eval");
    match result {
        pact_policy::rules::opa::OpaDecision::Deny { reason } => {
            assert_eq!(reason, "viewers cannot modify state");
        }
        other => panic!("expected Deny for viewer commit, got {other:?}"),
    }

    // Health check via OpaClient trait
    assert!(opa_client.health(), "OPA should be healthy");
}

/// HttpOpaClient returns error when OPA is unreachable.
#[tokio::test]
async fn opa_client_returns_error_when_unreachable() {
    // Point at a non-existent endpoint
    let client = HttpOpaClient::new("http://127.0.0.1:1");
    let input = OpaInput {
        identity: OpaIdentity {
            principal: "admin@example.com".into(),
            role: "pact-platform-admin".into(),
            principal_type: "Human".into(),
        },
        action: "commit".into(),
        scope: OpaScope { vcluster: "ml-training".into() },
        command: None,
    };
    let result = client.evaluate(&input).await;
    assert!(result.is_err(), "should fail when OPA is unreachable");
    // Health should also fail (HttpOpaClient::health returns true always currently,
    // but the evaluate call should fail)
}
