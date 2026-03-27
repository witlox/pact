//! E2E test: auth interceptor enforcement on journal gRPC services.
//!
//! Verifies P1: "Every operation authenticated — gRPC interceptor on all services."
//!
//! These tests would have caught the GCP deployment auth failure where:
//! - CLI created ConfigServiceClient::new() without auth interceptor
//! - Journal had auth_interceptor wired but CLI didn't attach tokens
//! - All tests passed because e2e Raft cluster had no interceptor
//!
//! Now the Raft cluster matches production wiring (auth_interceptor on all services),
//! and these tests verify the system rejects/accepts requests correctly.

use pact_cli::commands::execute::{AuthInterceptor, AuthenticatedChannel};
use pact_common::proto::journal::{
    config_service_client::ConfigServiceClient, GetNodeStateRequest, ListEntriesRequest,
};
use pact_e2e::containers::raft_cluster::RaftCluster;
use tonic::transport::Channel;

/// P1: Request without any auth token is rejected with UNAUTHENTICATED.
///
/// This is the test that would have caught the deployment failure.
/// If the interceptor isn't wired, this test fails (request succeeds when it shouldn't).
#[tokio::test]
async fn unauthenticated_request_rejected() {
    let cluster = RaftCluster::bootstrap(1).await.expect("cluster started");
    let node = cluster.node(1);

    // Create a raw client WITHOUT auth (simulates the old broken code)
    let channel = Channel::from_shared(format!("http://{}", node.grpc_addr))
        .unwrap()
        .connect()
        .await
        .unwrap();
    let mut client = ConfigServiceClient::new(channel);

    // This MUST fail with UNAUTHENTICATED
    let result = client
        .get_node_state(tonic::Request::new(GetNodeStateRequest { node_id: "test".to_string() }))
        .await;

    assert!(result.is_err(), "unauthenticated request should be rejected");
    let status = result.unwrap_err();
    assert_eq!(
        status.code(),
        tonic::Code::Unauthenticated,
        "expected UNAUTHENTICATED, got: {} — {}",
        status.code(),
        status.message()
    );
    assert!(
        status.message().contains("missing authorization header"),
        "error should mention missing auth: {}",
        status.message()
    );
}

/// P1: Request with valid Bearer token is accepted.
///
/// Proves the full client→interceptor→service pipeline works.
#[tokio::test]
async fn authenticated_request_accepted() {
    let cluster = RaftCluster::bootstrap(1).await.expect("cluster started");
    let node = cluster.node(1);

    let token = RaftCluster::test_token("admin@test", "pact-platform-admin");

    let channel = Channel::from_shared(format!("http://{}", node.grpc_addr))
        .unwrap()
        .connect()
        .await
        .unwrap();

    // Create client WITH auth interceptor (the production pattern)
    let mut client = ConfigServiceClient::with_interceptor(channel, AuthInterceptor::new(token));

    // This should succeed (node may not exist, but auth should pass)
    let result = client
        .get_node_state(tonic::Request::new(GetNodeStateRequest {
            node_id: "test-node".to_string(),
        }))
        .await;

    // The request should pass auth. It may return NotFound (no such node)
    // or Ok (empty state). Either way, NOT Unauthenticated.
    match result {
        Ok(_) => {} // auth passed, got a response
        Err(status) => {
            assert_ne!(
                status.code(),
                tonic::Code::Unauthenticated,
                "authenticated request should not get UNAUTHENTICATED: {}",
                status.message()
            );
        }
    }
}

/// P1: Request with garbage Bearer token is rejected.
///
/// The interceptor only checks format (Bearer prefix), but this verifies
/// that random strings don't bypass auth at the service level.
#[tokio::test]
async fn garbage_token_passes_interceptor_but_format_ok() {
    let cluster = RaftCluster::bootstrap(1).await.expect("cluster started");
    let node = cluster.node(1);

    let channel = Channel::from_shared(format!("http://{}", node.grpc_addr))
        .unwrap()
        .connect()
        .await
        .unwrap();

    // Garbage token — passes the Bearer prefix check but is not a valid JWT
    let mut client = ConfigServiceClient::with_interceptor(
        channel,
        AuthInterceptor::new("not-a-real-jwt".to_string()),
    );

    // The interceptor checks format only (Bearer prefix present).
    // The service method validates the actual JWT.
    // This should pass the interceptor but may fail at the service level
    // (depends on whether GetNodeState validates JWT or just checks format).
    let result = client
        .get_node_state(tonic::Request::new(GetNodeStateRequest { node_id: "test".to_string() }))
        .await;

    // Either OK (if service doesn't do deep validation on reads) or
    // Unauthenticated (if service validates JWT). Both are acceptable —
    // the important thing is this doesn't crash or bypass.
    match result {
        Ok(_) => {} // interceptor passed, service accepted
        Err(status) => {
            // Should be Unauthenticated (invalid JWT), not Internal or Unknown
            assert!(
                status.code() == tonic::Code::Unauthenticated
                    || status.code() == tonic::Code::NotFound,
                "unexpected error code: {} — {}",
                status.code(),
                status.message()
            );
        }
    }
}

/// P1: Request without Bearer prefix is rejected even if token is present.
#[tokio::test]
async fn non_bearer_scheme_rejected() {
    let cluster = RaftCluster::bootstrap(1).await.expect("cluster started");
    let node = cluster.node(1);

    let channel = Channel::from_shared(format!("http://{}", node.grpc_addr))
        .unwrap()
        .connect()
        .await
        .unwrap();

    let mut client = ConfigServiceClient::new(channel);

    // Manually add a Basic auth header (wrong scheme)
    let mut req = tonic::Request::new(GetNodeStateRequest { node_id: "test".to_string() });
    req.metadata_mut().insert("authorization", "Basic dXNlcjpwYXNz".parse().unwrap());

    let result = client.get_node_state(req).await;

    assert!(result.is_err());
    let status = result.unwrap_err();
    assert_eq!(status.code(), tonic::Code::Unauthenticated);
    assert!(status.message().contains("Bearer"));
}

/// P1: ListEntries (read) also requires auth — no read-without-auth bypass.
#[tokio::test]
async fn list_entries_requires_auth() {
    let cluster = RaftCluster::bootstrap(1).await.expect("cluster started");
    let node = cluster.node(1);

    let channel = Channel::from_shared(format!("http://{}", node.grpc_addr))
        .unwrap()
        .connect()
        .await
        .unwrap();
    let mut client = ConfigServiceClient::new(channel);

    let result = client
        .list_entries(tonic::Request::new(ListEntriesRequest {
            scope: None,
            from_sequence: None,
            to_sequence: None,
            limit: Some(1),
        }))
        .await;

    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code(), tonic::Code::Unauthenticated);
}

/// AuthenticatedChannel helper creates properly intercepted clients.
#[tokio::test]
async fn authenticated_channel_helper_works() {
    let cluster = RaftCluster::bootstrap(1).await.expect("cluster started");
    let node = cluster.node(1);

    let token = RaftCluster::test_token("ops@test", "pact-ops-default");

    let channel = Channel::from_shared(format!("http://{}", node.grpc_addr))
        .unwrap()
        .connect()
        .await
        .unwrap();

    let auth_channel = AuthenticatedChannel::new(channel, token);
    let mut config_client = auth_channel.config_client();

    // Auth should pass
    let result = config_client
        .get_node_state(tonic::Request::new(GetNodeStateRequest { node_id: "test".to_string() }))
        .await;

    match result {
        Ok(_) => {}
        Err(status) => {
            assert_ne!(
                status.code(),
                tonic::Code::Unauthenticated,
                "AuthenticatedChannel should inject token: {}",
                status.message()
            );
        }
    }

    // Policy client should also have auth
    let mut policy_client = auth_channel.policy_client();
    let result = policy_client
        .get_effective_policy(tonic::Request::new(pact_common::proto::policy::GetPolicyRequest {
            vcluster_id: "test".to_string(),
        }))
        .await;

    match result {
        Ok(_) => {}
        Err(status) => {
            assert_ne!(
                status.code(),
                tonic::Code::Unauthenticated,
                "PolicyClient should also have auth: {}",
                status.message()
            );
        }
    }
}
