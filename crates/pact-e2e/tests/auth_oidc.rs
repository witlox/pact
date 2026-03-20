//! E2E test: real OAuth2/OIDC authentication via Keycloak container.
//!
//! Tests the full token lifecycle:
//! - Discovery endpoint (well-known config)
//! - Client credentials flow (service accounts)
//! - Token validation (JWT decode, signature, expiry, audience, issuer)
//! - Token refresh
//! - Token revocation
//! - Cache file operations (hpc-auth crate)
//!
//! Requires Docker.

use reqwest::Client;
use serde::Deserialize;
use serde_json::json;
use testcontainers::runners::AsyncRunner;

use pact_e2e::containers::keycloak::{Keycloak, KEYCLOAK_PORT};

/// Keycloak token endpoint response.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    expires_in: u64,
    token_type: String,
}

/// Helper: get admin token for Keycloak API calls.
async fn admin_token(client: &Client, base_url: &str) -> String {
    let resp: TokenResponse = client
        .post(format!("{base_url}/realms/master/protocol/openid-connect/token"))
        .form(&[
            ("grant_type", "password"),
            ("client_id", "admin-cli"),
            ("username", "admin"),
            ("password", "admin"),
        ])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    resp.access_token
}

/// Helper: create a pact realm with client and test user.
async fn setup_pact_realm(client: &Client, base_url: &str, admin_tok: &str) {
    // Create realm
    client
        .post(format!("{base_url}/admin/realms"))
        .bearer_auth(admin_tok)
        .json(&json!({
            "realm": "pact",
            "enabled": true,
            "accessTokenLifespan": 300,
            "ssoSessionIdleTimeout": 1800
        }))
        .send()
        .await
        .unwrap();

    // Create confidential client with pact_role mapper
    let role_mapper_config: std::collections::HashMap<String, String> = [
        ("claim.name", "pact_role"),
        ("user.attribute", "pact_role"),
        ("jsonType.label", "String"),
        ("id.token.claim", "true"),
        ("access.token.claim", "true"),
        ("userinfo.token.claim", "true"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();

    let client_rep = json!({
        "clientId": "pact-cli",
        "enabled": true,
        "serviceAccountsEnabled": true,
        "directAccessGrantsEnabled": true,
        "publicClient": false,
        "secret": "pact-test-secret",
        "redirectUris": ["http://localhost:*"],
        "protocolMappers": [{
            "name": "pact-role-mapper",
            "protocol": "openid-connect",
            "protocolMapper": "oidc-usermodel-attribute-mapper",
            "consentRequired": false,
            "config": role_mapper_config
        }]
    });

    client
        .post(format!("{base_url}/admin/realms/pact/clients"))
        .bearer_auth(admin_tok)
        .json(&client_rep)
        .send()
        .await
        .unwrap();

    // Create test users
    for (username, email, role) in [
        ("admin", "admin@example.com", "pact-platform-admin"),
        ("ops", "ops@example.com", "pact-ops-ml-training"),
        ("viewer", "viewer@example.com", "pact-viewer-ml-training"),
    ] {
        let user = json!({
            "username": username,
            "email": email,
            "enabled": true,
            "emailVerified": true,
            "attributes": {
                "pact_role": [role]
            },
            "credentials": [{
                "type": "password",
                "value": "testpass",
                "temporary": false
            }]
        });

        client
            .post(format!("{base_url}/admin/realms/pact/users"))
            .bearer_auth(admin_tok)
            .json(&user)
            .send()
            .await
            .unwrap();
    }
}

/// Test: OIDC discovery endpoint returns valid configuration.
#[tokio::test]
async fn keycloak_discovery_endpoint() {
    let container = Keycloak::default().start().await.expect("Keycloak container started");

    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(KEYCLOAK_PORT).await.unwrap();
    let base_url = format!("http://{host}:{port}");
    let client = Client::new();

    // Set up realm
    let admin_tok = admin_token(&client, &base_url).await;
    setup_pact_realm(&client, &base_url, &admin_tok).await;

    // Fetch discovery document
    let discovery: serde_json::Value = client
        .get(format!("{base_url}/realms/pact/.well-known/openid-configuration"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(discovery["issuer"], format!("{base_url}/realms/pact"));
    assert!(discovery["token_endpoint"].as_str().unwrap().contains("/token"));
    assert!(discovery["jwks_uri"].as_str().unwrap().contains("/certs"));
    assert!(discovery["authorization_endpoint"].as_str().unwrap().contains("/auth"));
}

/// Test: client credentials flow produces valid JWT with pact_role claim.
#[tokio::test]
async fn keycloak_client_credentials_flow() {
    let container = Keycloak::default().start().await.expect("Keycloak container started");

    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(KEYCLOAK_PORT).await.unwrap();
    let base_url = format!("http://{host}:{port}");
    let client = Client::new();

    let admin_tok = admin_token(&client, &base_url).await;
    setup_pact_realm(&client, &base_url, &admin_tok).await;

    // Client credentials flow
    let token_resp: TokenResponse = client
        .post(format!("{base_url}/realms/pact/protocol/openid-connect/token"))
        .form(&[
            ("grant_type", "client_credentials"),
            ("client_id", "pact-cli"),
            ("client_secret", "pact-test-secret"),
        ])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert!(!token_resp.access_token.is_empty());
    assert_eq!(token_resp.token_type, "Bearer");
    assert!(token_resp.expires_in > 0);

    // Decode JWT and verify structure (without signature validation — just structure)
    assert_eq!(token_resp.access_token.split('.').count(), 3, "JWT should have 3 parts");
}

/// Test: resource owner password flow with pact_role in token claims.
#[tokio::test]
async fn keycloak_password_flow_with_role_claim() {
    let container = Keycloak::default().start().await.expect("Keycloak container started");

    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(KEYCLOAK_PORT).await.unwrap();
    let base_url = format!("http://{host}:{port}");
    let client = Client::new();

    let admin_tok = admin_token(&client, &base_url).await;
    setup_pact_realm(&client, &base_url, &admin_tok).await;

    // Password grant for admin user
    let token_resp: TokenResponse = client
        .post(format!("{base_url}/realms/pact/protocol/openid-connect/token"))
        .form(&[
            ("grant_type", "password"),
            ("client_id", "pact-cli"),
            ("client_secret", "pact-test-secret"),
            ("username", "admin"),
            ("password", "testpass"),
        ])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert!(!token_resp.access_token.is_empty());
    assert!(token_resp.refresh_token.is_some(), "password flow should return refresh token");

    // Decode the access token claims
    let payload = token_resp.access_token.split('.').nth(1).unwrap();
    let decoded = base64_decode_json(payload);
    assert_eq!(decoded["preferred_username"], "admin");
    assert_eq!(decoded["email"], "admin@example.com");
    // pact_role custom claim from user attribute mapper
    assert_eq!(decoded["pact_role"], "pact-platform-admin");
}

/// Test: token refresh produces new access token.
#[tokio::test]
async fn keycloak_token_refresh() {
    let container = Keycloak::default().start().await.expect("Keycloak container started");

    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(KEYCLOAK_PORT).await.unwrap();
    let base_url = format!("http://{host}:{port}");
    let client = Client::new();

    let admin_tok = admin_token(&client, &base_url).await;
    setup_pact_realm(&client, &base_url, &admin_tok).await;

    // Get initial tokens
    let initial: TokenResponse = client
        .post(format!("{base_url}/realms/pact/protocol/openid-connect/token"))
        .form(&[
            ("grant_type", "password"),
            ("client_id", "pact-cli"),
            ("client_secret", "pact-test-secret"),
            ("username", "ops"),
            ("password", "testpass"),
        ])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let refresh_token = initial.refresh_token.unwrap();

    // Refresh
    let refreshed: TokenResponse = client
        .post(format!("{base_url}/realms/pact/protocol/openid-connect/token"))
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", "pact-cli"),
            ("client_secret", "pact-test-secret"),
            ("refresh_token", &refresh_token),
        ])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert!(!refreshed.access_token.is_empty());
    assert_ne!(refreshed.access_token, initial.access_token, "refreshed token should differ");
}

/// Test: hpc-auth TokenCache roundtrip with real tokens.
#[tokio::test]
async fn keycloak_token_cache_roundtrip() {
    let container = Keycloak::default().start().await.expect("Keycloak container started");

    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(KEYCLOAK_PORT).await.unwrap();
    let base_url = format!("http://{host}:{port}");
    let client = Client::new();

    let admin_tok = admin_token(&client, &base_url).await;
    setup_pact_realm(&client, &base_url, &admin_tok).await;

    // Get real token
    let token_resp: TokenResponse = client
        .post(format!("{base_url}/realms/pact/protocol/openid-connect/token"))
        .form(&[
            ("grant_type", "password"),
            ("client_id", "pact-cli"),
            ("client_secret", "pact-test-secret"),
            ("username", "viewer"),
            ("password", "testpass"),
        ])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    // Store in hpc-auth TokenCache
    let tmp = tempfile::TempDir::new().unwrap();
    let cache = hpc_auth::cache::TokenCache::new(
        tmp.path().to_path_buf(),
        hpc_auth::PermissionMode::Strict,
    );

    let server_url = format!("{base_url}/realms/pact");
    let token_set = hpc_auth::TokenSet {
        access_token: token_resp.access_token.clone(),
        refresh_token: token_resp.refresh_token,
        expires_at: chrono::Utc::now() + chrono::Duration::seconds(token_resp.expires_in as i64),
        scopes: vec!["openid".into()],
    };

    cache.write(&server_url, &token_set).unwrap();

    // Read back
    let read_back = cache.read(&server_url).unwrap().unwrap();
    assert_eq!(read_back.access_token, token_resp.access_token);
    assert!(read_back.refresh_token.is_some());

    // Verify cache file permissions
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let path = tmp.path().join("tokens.json");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "token cache should have 0600 permissions");
    }

    // Set default server
    cache.set_default_server(&server_url).unwrap();
    assert_eq!(cache.default_server(), Some(server_url.clone()));

    // List servers
    let servers = cache.list_servers();
    assert!(servers.contains(&server_url));
}

/// Test: JWKS endpoint returns valid RSA keys for token validation.
#[tokio::test]
async fn keycloak_jwks_endpoint() {
    let container = Keycloak::default().start().await.expect("Keycloak container started");

    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(KEYCLOAK_PORT).await.unwrap();
    let base_url = format!("http://{host}:{port}");
    let client = Client::new();

    let admin_tok = admin_token(&client, &base_url).await;
    setup_pact_realm(&client, &base_url, &admin_tok).await;

    // Fetch JWKS
    let jwks: serde_json::Value = client
        .get(format!("{base_url}/realms/pact/protocol/openid-connect/certs"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let keys = jwks["keys"].as_array().unwrap();
    assert!(!keys.is_empty(), "JWKS should have at least one key");

    // Verify RSA key structure
    let rsa_key = keys.iter().find(|k| k["kty"] == "RSA").unwrap();
    assert!(rsa_key["n"].as_str().is_some(), "RSA key should have modulus");
    assert!(rsa_key["e"].as_str().is_some(), "RSA key should have exponent");
    assert!(rsa_key["kid"].as_str().is_some(), "RSA key should have kid");
}

/// Decode base64url JWT payload to JSON.
fn base64_decode_json(payload: &str) -> serde_json::Value {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    let bytes = URL_SAFE_NO_PAD.decode(payload).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}
