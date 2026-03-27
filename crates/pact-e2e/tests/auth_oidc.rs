//! E2E test: real OIDC authentication via Dex container.
//!
//! Tests the OIDC token lifecycle: discovery, JWKS, password flow, token
//! refresh, and hpc-auth cache integration. Uses Dex (~2-3s startup) instead
//! of Keycloak (~60-180s) for fast CI feedback.
//!
//! Requires Docker.

use std::time::Duration;

use reqwest::Client;
use serde::Deserialize;
use testcontainers::runners::AsyncRunner;
use testcontainers::ImageExt;

use pact_e2e::containers::dex::{dex_test_config, Dex, DEX_PORT};
use pact_policy::iam::TokenValidator;
use testcontainers::core::CopyDataSource;

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: u64,
    token_type: String,
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn dex_oidc_lifecycle() {
    // Dex needs a config with the correct issuer URL, but we don't know
    // the mapped port yet. Use a placeholder issuer — Dex is lenient
    // about issuer mismatch in dev mode.
    let config = dex_test_config("http://127.0.0.1:5556/dex");
    let container = Dex::default()
        .with_copy_to("/etc/dex/config.yaml", CopyDataSource::Data(config.into_bytes()))
        .with_startup_timeout(Duration::from_secs(30))
        .start()
        .await
        .expect("Dex container started");

    let host = container.get_host().await.unwrap();
    let port = container.get_host_port_ipv4(DEX_PORT).await.unwrap();
    let base_url = format!("http://{host}:{port}");
    let client = Client::new();

    // === OIDC discovery ===
    let discovery: serde_json::Value = client
        .get(format!("{base_url}/dex/.well-known/openid-configuration"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert!(discovery["issuer"].as_str().is_some(), "discovery should have issuer");
    assert!(
        discovery["token_endpoint"].as_str().unwrap().contains("/token"),
        "discovery should have token_endpoint"
    );
    assert!(
        discovery["jwks_uri"].as_str().unwrap().contains("/keys"),
        "discovery should have jwks_uri"
    );

    // === JWKS endpoint ===
    let jwks: serde_json::Value =
        client.get(format!("{base_url}/dex/keys")).send().await.unwrap().json().await.unwrap();

    let keys = jwks["keys"].as_array().expect("JWKS should have keys array");
    assert!(!keys.is_empty(), "JWKS should have at least one key");
    assert!(keys.iter().any(|k| k["kty"] == "RSA"), "JWKS should have an RSA key");

    // === Password grant (Resource Owner) ===
    // Discovery returns the internal container URL; rewrite to use mapped port.
    let token_endpoint = format!("{base_url}/dex/token");
    let pw: TokenResponse = client
        .post(&token_endpoint)
        .form(&[
            ("grant_type", "password"),
            ("client_id", "pact-cli"),
            ("client_secret", "pact-test-secret"),
            ("username", "admin@pact.test"),
            ("password", "password"),
            ("scope", "openid email profile offline_access"),
        ])
        .send()
        .await
        .unwrap()
        .error_for_status()
        .expect("password flow should succeed")
        .json()
        .await
        .unwrap();

    assert!(!pw.access_token.is_empty(), "should get an access token");
    assert_eq!(pw.token_type, "bearer", "token type should be bearer");
    assert!(pw.refresh_token.is_some(), "should get a refresh token");

    // Decode JWT payload to verify claims
    let payload = pw.access_token.split('.').nth(1).unwrap();
    let decoded = {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        serde_json::from_slice::<serde_json::Value>(&URL_SAFE_NO_PAD.decode(payload).unwrap())
            .unwrap()
    };
    assert_eq!(decoded["email"], "admin@pact.test", "email claim");

    // === Token refresh ===
    let refreshed: TokenResponse = client
        .post(&token_endpoint)
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", "pact-cli"),
            ("client_secret", "pact-test-secret"),
            ("refresh_token", pw.refresh_token.as_ref().unwrap()),
        ])
        .send()
        .await
        .unwrap()
        .error_for_status()
        .expect("refresh flow should succeed")
        .json()
        .await
        .unwrap();

    assert!(!refreshed.access_token.is_empty());
    assert_ne!(refreshed.access_token, pw.access_token, "refreshed token should be different");

    // === hpc-auth TokenCache roundtrip ===
    let tmp = tempfile::TempDir::new().unwrap();
    let cache = hpc_auth::cache::TokenCache::new(
        tmp.path().to_path_buf(),
        hpc_auth::PermissionMode::Strict,
    );
    let server_url = format!("{base_url}/dex");
    let token_set = hpc_auth::TokenSet {
        access_token: pw.access_token.clone(),
        refresh_token: pw.refresh_token,
        expires_at: chrono::Utc::now() + chrono::Duration::seconds(pw.expires_in as i64),
        scopes: vec!["openid".into()],
    };
    cache.write(&server_url, &token_set).unwrap();
    let read_back = cache.read(&server_url).unwrap().unwrap();
    assert_eq!(read_back.access_token, pw.access_token);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode =
            std::fs::metadata(tmp.path().join("tokens.json")).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "token cache should be 0600");
    }

    cache.set_default_server(&server_url).unwrap();
    assert_eq!(cache.default_server(), Some(server_url));

    // === JwksTokenValidator validates the Dex-issued token ===
    // Use the internal issuer (matches what's inside the JWT) but mapped JWKS URL.
    let issuer = discovery["issuer"].as_str().unwrap().to_string();
    let jwks_url = format!("{base_url}/dex/keys");
    let validator = pact_policy::iam::JwksTokenValidator::new(
        pact_policy::iam::OidcConfig {
            issuer: issuer.clone(),
            audience: "pact-cli".to_string(),
            hmac_secret: None,
        },
        Some(jwks_url),
    );

    let identity = validator
        .validate(&pw.access_token)
        .await
        .expect("JwksTokenValidator should validate Dex-issued RS256 token");
    // Dex uses a base64-encoded composite subject, not the raw userID.
    // Just verify the principal is non-empty — the exact format is Dex-specific.
    assert!(!identity.principal.is_empty(), "identity principal should be non-empty");
}
