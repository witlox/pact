use std::collections::HashMap;
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{self, Utc};
use rand::Rng;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::io::AsyncBufReadExt;
use tracing::{debug, info};

use crate::error::AuthError;
use crate::types::{OidcDiscovery, TokenSet};

/// OAuth2 token response from the IdP.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    scope: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    token_type: String,
}

impl TokenResponse {
    fn into_token_set(self) -> TokenSet {
        let expires_at =
            Utc::now() + chrono::Duration::seconds(self.expires_in.unwrap_or(3600) as i64);
        let scopes = self
            .scope
            .map(|s| s.split_whitespace().map(String::from).collect())
            .unwrap_or_default();
        TokenSet {
            access_token: self.access_token,
            refresh_token: self.refresh_token,
            expires_at,
            scopes,
        }
    }
}

/// Device authorization response from the IdP.
#[derive(Debug, Deserialize)]
struct DeviceAuthResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    #[serde(default = "default_interval")]
    interval: u64,
}

fn default_interval() -> u64 {
    5
}

/// Error response from token endpoint during device code polling.
#[derive(Debug, Deserialize)]
struct OAuthErrorResponse {
    error: String,
    #[serde(default)]
    #[allow(dead_code)]
    error_description: String,
}

/// Generate a cryptographically random PKCE code verifier (43-128 chars, URL-safe base64).
pub fn generate_code_verifier() -> String {
    let mut rng = rand::rng();
    let len: usize = rng.random_range(32..=96);
    let bytes: Vec<u8> = (0..len).map(|_| rng.random()).collect();
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Compute the S256 PKCE code challenge from a code verifier.
pub fn compute_code_challenge(verifier: &str) -> String {
    let digest = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(digest)
}

/// Generate a random state parameter for CSRF protection.
fn generate_state() -> String {
    let bytes: [u8; 16] = rand::random();
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Authorization Code with PKCE flow (RFC 7636).
///
/// Opens the user's browser for IdP login, listens on a localhost callback,
/// then exchanges the authorization code for tokens.
pub async fn auth_code_pkce(
    discovery: &OidcDiscovery,
    client_id: &str,
    timeout: Duration,
) -> Result<TokenSet, AuthError> {
    // 1. Generate PKCE parameters.
    let code_verifier = generate_code_verifier();
    let code_challenge = compute_code_challenge(&code_verifier);
    let state = generate_state();

    // 2. Start localhost TCP listener on an ephemeral port.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| AuthError::Internal(format!("failed to bind callback listener: {e}")))?;
    let local_addr = listener
        .local_addr()
        .map_err(|e| AuthError::Internal(format!("failed to get listener address: {e}")))?;
    let port = local_addr.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");

    // 3. Build the authorization URL.
    let auth_url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&code_challenge={}&code_challenge_method=S256&scope=openid+profile&state={}",
        discovery.authorization_endpoint,
        urlencoding::encode(client_id),
        urlencoding::encode(&redirect_uri),
        urlencoding::encode(&code_challenge),
        urlencoding::encode(&state),
    );

    // 4. Open browser (best-effort).
    open_browser(&auth_url);

    // 5. Wait for the callback with a timeout.
    let auth_code = tokio::time::timeout(timeout, wait_for_callback(listener, &state))
        .await
        .map_err(|_| AuthError::Timeout)?
        .map_err(|e| AuthError::OAuthFailed(format!("callback failed: {e}")))?;

    // 6. Exchange the code for tokens.
    let client = reqwest::Client::new();
    let mut params = HashMap::new();
    params.insert("grant_type", "authorization_code");
    params.insert("code", &auth_code);
    params.insert("redirect_uri", &redirect_uri);
    params.insert("client_id", client_id);
    params.insert("code_verifier", &code_verifier);

    let response = client
        .post(&discovery.token_endpoint)
        .form(&params)
        .send()
        .await
        .map_err(|e| AuthError::IdpUnreachable(format!("{}: {e}", discovery.token_endpoint)))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AuthError::OAuthFailed(format!(
            "token exchange failed: HTTP {status}: {body}"
        )));
    }

    let token_response: TokenResponse = response
        .json()
        .await
        .map_err(|e| AuthError::OAuthFailed(format!("invalid token response: {e}")))?;

    Ok(token_response.into_token_set())
}

/// Wait for the OAuth2 callback on the localhost listener.
///
/// Reads the HTTP request, extracts the `code` and `state` query parameters,
/// sends back a simple HTML response, and returns the authorization code.
async fn wait_for_callback(
    listener: tokio::net::TcpListener,
    expected_state: &str,
) -> Result<String, String> {
    use tokio::io::AsyncReadExt;
    use tokio::io::AsyncWriteExt;

    let (mut stream, _addr) = listener.accept().await.map_err(|e| format!("accept failed: {e}"))?;

    // Read the HTTP request (we only need the first line for the GET path).
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.map_err(|e| format!("read failed: {e}"))?;
    let request = String::from_utf8_lossy(&buf[..n]);

    // Parse the request line: GET /callback?code=...&state=... HTTP/1.1
    let request_line = request.lines().next().unwrap_or("");
    let path = request_line.split_whitespace().nth(1).unwrap_or("");

    // Extract query parameters.
    let query = path.split('?').nth(1).unwrap_or("");
    let params: HashMap<&str, &str> = query
        .split('&')
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            Some((parts.next()?, parts.next()?))
        })
        .collect();

    // Check for error response from IdP.
    if let Some(error) = params.get("error") {
        let desc = params.get("error_description").unwrap_or(&"");
        let body = format!(
            "<html><body><h1>Authentication Failed</h1><p>{error}: {desc}</p></body></html>"
        );
        let response = format!(
            "HTTP/1.1 400 Bad Request\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(response.as_bytes()).await;
        return Err(format!("IdP returned error: {error}: {desc}"));
    }

    // Validate state parameter (CSRF protection).
    let received_state = params.get("state").ok_or("missing state parameter")?;
    if *received_state != expected_state {
        return Err("state parameter mismatch (possible CSRF)".to_string());
    }

    let code = params.get("code").ok_or("missing code parameter")?.to_string();

    // Send success response.
    let body =
        "<html><body><h1>Authentication Successful</h1><p>You can close this window.</p></body></html>";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    let _ = stream.write_all(response.as_bytes()).await;

    Ok(code)
}

/// Open a URL in the user's default browser.
///
/// Falls back to printing the URL on stderr if browser launch fails.
fn open_browser(url: &str) {
    let result = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).spawn()
    } else if cfg!(target_os = "linux") {
        std::process::Command::new("xdg-open").arg(url).spawn()
    } else {
        Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "unsupported platform"))
    };

    match result {
        Ok(_) => {
            info!("opened browser for authentication");
        }
        Err(_) => {
            eprintln!("\nOpen this URL in your browser to authenticate:\n\n  {url}\n");
        }
    }
}

/// Device Authorization Grant flow (RFC 8628).
///
/// Displays a user code, then polls the token endpoint until the user authorizes.
pub async fn device_code(
    discovery: &OidcDiscovery,
    client_id: &str,
    timeout: Duration,
) -> Result<TokenSet, AuthError> {
    let device_endpoint = discovery.device_authorization_endpoint.as_deref().ok_or_else(|| {
        AuthError::OAuthFailed("no device_authorization_endpoint in discovery".to_string())
    })?;

    // 1. Request device authorization.
    let http = reqwest::Client::new();
    let mut params = HashMap::new();
    params.insert("client_id", client_id);
    params.insert("scope", "openid profile");

    let response = http
        .post(device_endpoint)
        .form(&params)
        .send()
        .await
        .map_err(|e| AuthError::IdpUnreachable(format!("{device_endpoint}: {e}")))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AuthError::OAuthFailed(format!(
            "device authorization failed: HTTP {status}: {body}"
        )));
    }

    let device_auth: DeviceAuthResponse = response
        .json()
        .await
        .map_err(|e| AuthError::OAuthFailed(format!("invalid device auth response: {e}")))?;

    // 2. Display instructions to the user.
    eprintln!(
        "\nTo authenticate, visit:\n\n  {}\n\nand enter code: {}\n",
        device_auth.verification_uri, device_auth.user_code
    );

    // 3. Poll for token, respecting interval and timeout.
    let mut interval_secs = device_auth.interval.max(1);
    let effective_timeout = Duration::from_secs(device_auth.expires_in).min(timeout);
    let deadline = tokio::time::Instant::now() + effective_timeout;

    loop {
        tokio::time::sleep(Duration::from_secs(interval_secs)).await;

        if tokio::time::Instant::now() >= deadline {
            return Err(AuthError::Timeout);
        }

        let mut poll_params = HashMap::new();
        poll_params.insert("grant_type", "urn:ietf:params:oauth:grant-type:device_code");
        poll_params.insert("device_code", &device_auth.device_code);
        poll_params.insert("client_id", client_id);

        let poll_response =
            http.post(&discovery.token_endpoint).form(&poll_params).send().await.map_err(|e| {
                AuthError::IdpUnreachable(format!("{}: {e}", discovery.token_endpoint))
            })?;

        if poll_response.status().is_success() {
            let token_response: TokenResponse = poll_response
                .json()
                .await
                .map_err(|e| AuthError::OAuthFailed(format!("invalid token response: {e}")))?;
            return Ok(token_response.into_token_set());
        }

        // Parse error response to determine next action.
        let body = poll_response.text().await.unwrap_or_default();
        let error_resp: Result<OAuthErrorResponse, _> = serde_json::from_str(&body);

        match error_resp {
            Ok(err) => match err.error.as_str() {
                "authorization_pending" => {
                    debug!("device code: authorization pending, continuing to poll");
                }
                "slow_down" => {
                    interval_secs += 5;
                    debug!(interval = interval_secs, "device code: slowing down poll interval");
                }
                "expired_token" => {
                    return Err(AuthError::Timeout);
                }
                "access_denied" => {
                    return Err(AuthError::OAuthFailed("user denied access".to_string()));
                }
                other => {
                    return Err(AuthError::OAuthFailed(format!("device code error: {other}")));
                }
            },
            Err(_) => {
                return Err(AuthError::OAuthFailed(format!(
                    "unexpected error response from token endpoint: {body}"
                )));
            }
        }
    }
}

/// Client Credentials flow (machine-to-machine).
///
/// This is the simplest OAuth2 flow -- a POST to the token endpoint.
pub async fn client_credentials(
    token_endpoint: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<TokenSet, AuthError> {
    let client = reqwest::Client::new();

    let mut params = HashMap::new();
    params.insert("grant_type", "client_credentials");
    params.insert("client_id", client_id);
    params.insert("client_secret", client_secret);

    let response = client
        .post(token_endpoint)
        .form(&params)
        .send()
        .await
        .map_err(|e| AuthError::IdpUnreachable(format!("{token_endpoint}: {e}")))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AuthError::OAuthFailed(format!(
            "client_credentials failed: HTTP {status}: {body}"
        )));
    }

    let token_response: TokenResponse = response
        .json()
        .await
        .map_err(|e| AuthError::OAuthFailed(format!("invalid token response: {e}")))?;

    Ok(token_response.into_token_set())
}

/// Manual token paste flow (for SSH sessions without browser).
///
/// Prints the authorization URL and reads the code from stdin.
pub async fn manual_paste(
    discovery: &OidcDiscovery,
    client_id: &str,
    timeout: Duration,
) -> Result<TokenSet, AuthError> {
    // 1. Generate PKCE parameters (use PKCE if the IdP supports it).
    let supports_pkce = discovery.code_challenge_methods_supported.contains(&"S256".to_string());

    let code_verifier = if supports_pkce { Some(generate_code_verifier()) } else { None };

    let state = generate_state();

    // We use a fixed redirect URI that expects manual paste.
    // The user will copy the code from the browser redirect or the IdP page.
    let redirect_uri = "urn:ietf:wg:oauth:2.0:oob";

    // 2. Build the authorization URL.
    let mut auth_url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope=openid+profile&state={}",
        discovery.authorization_endpoint,
        urlencoding::encode(client_id),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(&state),
    );

    if let Some(ref verifier) = code_verifier {
        let challenge = compute_code_challenge(verifier);
        auth_url.push_str(&format!(
            "&code_challenge={}&code_challenge_method=S256",
            urlencoding::encode(&challenge)
        ));
    }

    // 3. Print the URL and prompt for the code.
    eprintln!("\nOpen this URL in a browser:\n\n  {auth_url}\n");
    eprint!("Paste the authorization code here: ");

    // 4. Read the code from stdin with a timeout.
    let code = tokio::time::timeout(timeout, async {
        let stdin = tokio::io::stdin();
        let reader = tokio::io::BufReader::new(stdin);
        let mut lines = reader.lines();
        lines
            .next_line()
            .await
            .map_err(|e| AuthError::Internal(format!("failed to read from stdin: {e}")))?
            .ok_or_else(|| AuthError::Internal("stdin closed unexpectedly".to_string()))
    })
    .await
    .map_err(|_| AuthError::Timeout)??;

    let code = code.trim().to_string();
    if code.is_empty() {
        return Err(AuthError::OAuthFailed("empty authorization code".to_string()));
    }

    // 5. Exchange the code for tokens.
    let http = reqwest::Client::new();
    let mut params = HashMap::new();
    params.insert("grant_type", "authorization_code".to_string());
    params.insert("code", code);
    params.insert("redirect_uri", redirect_uri.to_string());
    params.insert("client_id", client_id.to_string());
    if let Some(verifier) = code_verifier {
        params.insert("code_verifier", verifier);
    }

    let response = http
        .post(&discovery.token_endpoint)
        .form(&params)
        .send()
        .await
        .map_err(|e| AuthError::IdpUnreachable(format!("{}: {e}", discovery.token_endpoint)))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AuthError::OAuthFailed(format!(
            "token exchange failed: HTTP {status}: {body}"
        )));
    }

    let token_response: TokenResponse = response
        .json()
        .await
        .map_err(|e| AuthError::OAuthFailed(format!("invalid token response: {e}")))?;

    Ok(token_response.into_token_set())
}

/// Refresh token flow.
pub async fn refresh_token(
    token_endpoint: &str,
    refresh_tok: &str,
    client_id: &str,
) -> Result<TokenSet, AuthError> {
    let client = reqwest::Client::new();

    let mut params = HashMap::new();
    params.insert("grant_type", "refresh_token");
    params.insert("refresh_token", refresh_tok);
    params.insert("client_id", client_id);

    let response = client
        .post(token_endpoint)
        .form(&params)
        .send()
        .await
        .map_err(|e| AuthError::IdpUnreachable(format!("{token_endpoint}: {e}")))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(AuthError::OAuthFailed(format!("refresh_token failed: HTTP {status}: {body}")));
    }

    let token_response: TokenResponse = response
        .json()
        .await
        .map_err(|e| AuthError::OAuthFailed(format!("invalid token response: {e}")))?;

    Ok(token_response.into_token_set())
}

/// Discover IdP configuration from the pact journal server.
///
/// Fetches `{server_url}/auth/discovery` to get the IdP URL and client ID,
/// then fetches OIDC discovery from the IdP.
pub async fn server_discovery(
    server_url: &str,
    timeout: Duration,
) -> Result<(OidcDiscovery, String), AuthError> {
    let http = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .map_err(|e| AuthError::Internal(format!("http client error: {e}")))?;

    // 1. Fetch journal's auth discovery endpoint.
    let discovery_url = format!("{}/auth/discovery", server_url.trim_end_matches('/'));
    let response = http
        .get(&discovery_url)
        .send()
        .await
        .map_err(|e| AuthError::IdpUnreachable(format!("{discovery_url}: {e}")))?;

    if !response.status().is_success() {
        return Err(AuthError::IdpUnreachable(format!(
            "{discovery_url}: HTTP {}",
            response.status()
        )));
    }

    let server_disc: ServerDiscoveryResponse = response
        .json()
        .await
        .map_err(|e| AuthError::Internal(format!("invalid server discovery response: {e}")))?;

    // 2. Fetch OIDC discovery from the IdP.
    let oidc_url =
        format!("{}/.well-known/openid-configuration", server_disc.idp_url.trim_end_matches('/'));
    let oidc_response = http
        .get(&oidc_url)
        .send()
        .await
        .map_err(|e| AuthError::IdpUnreachable(format!("{oidc_url}: {e}")))?;

    if !oidc_response.status().is_success() {
        return Err(AuthError::IdpUnreachable(format!(
            "{oidc_url}: HTTP {}",
            oidc_response.status()
        )));
    }

    let oidc_discovery: OidcDiscovery = oidc_response
        .json()
        .await
        .map_err(|e| AuthError::Internal(format!("invalid OIDC discovery document: {e}")))?;

    Ok((oidc_discovery, server_disc.client_id))
}

/// Response from the journal server's `/auth/discovery` endpoint.
#[derive(Debug, Deserialize)]
struct ServerDiscoveryResponse {
    idp_url: String,
    client_id: String,
}

/// Build an authorization URL (exposed for testing and manual_paste).
pub fn build_auth_url(
    authorization_endpoint: &str,
    client_id: &str,
    redirect_uri: &str,
    state: &str,
    code_challenge: Option<&str>,
    scope: &str,
) -> String {
    let mut url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}",
        authorization_endpoint,
        urlencoding::encode(client_id),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(scope),
        urlencoding::encode(state),
    );
    if let Some(challenge) = code_challenge {
        url.push_str(&format!(
            "&code_challenge={}&code_challenge_method=S256",
            urlencoding::encode(challenge)
        ));
    }
    url
}

/// URL-encode a string (minimal implementation for query parameters).
mod urlencoding {
    pub fn encode(s: &str) -> String {
        let mut result = String::with_capacity(s.len());
        for byte in s.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    result.push(byte as char);
                }
                _ => {
                    result.push_str(&format!("%{byte:02X}"));
                }
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_verifier_length_is_valid() {
        for _ in 0..20 {
            let verifier = generate_code_verifier();
            // Base64 of 32-96 bytes → 43-128 chars (URL_SAFE_NO_PAD)
            assert!(
                verifier.len() >= 43 && verifier.len() <= 128,
                "verifier length {} out of range",
                verifier.len()
            );
            // Only URL-safe base64 characters.
            assert!(verifier.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        }
    }

    #[test]
    fn code_challenge_is_deterministic() {
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = compute_code_challenge(verifier);
        // The challenge should be consistent for the same verifier.
        assert_eq!(challenge, compute_code_challenge(verifier));
        // It should be URL-safe base64 encoded.
        assert!(challenge.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
        // SHA-256 output is 32 bytes, base64(32) = 43 chars (no padding).
        assert_eq!(challenge.len(), 43);
    }

    #[test]
    fn code_challenge_matches_rfc7636_example() {
        // RFC 7636 Appendix B example:
        // code_verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"
        // code_challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = compute_code_challenge(verifier);
        assert_eq!(challenge, "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
    }

    #[test]
    fn generate_state_is_nonempty_and_urlsafe() {
        let state = generate_state();
        assert!(!state.is_empty());
        assert!(state.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn build_auth_url_without_pkce() {
        let url = build_auth_url(
            "https://idp.example.com/auth",
            "my-client",
            "http://127.0.0.1:8080/callback",
            "random-state",
            None,
            "openid profile",
        );
        assert!(url.starts_with("https://idp.example.com/auth?"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("client_id=my-client"));
        assert!(url.contains("state=random-state"));
        assert!(!url.contains("code_challenge"));
    }

    #[test]
    fn build_auth_url_with_pkce() {
        let challenge = compute_code_challenge("test-verifier");
        let url = build_auth_url(
            "https://idp.example.com/auth",
            "my-client",
            "http://127.0.0.1:8080/callback",
            "random-state",
            Some(&challenge),
            "openid profile",
        );
        assert!(url.contains("code_challenge="));
        assert!(url.contains("code_challenge_method=S256"));
    }

    #[test]
    fn device_auth_response_deserialization() {
        let json = r#"{
            "device_code": "GmRhmhcxhwAzkoEqiMEg_DnyEysNkuNhszIySk9eS",
            "user_code": "WDJB-MJHT",
            "verification_uri": "https://idp.example.com/device",
            "expires_in": 1800,
            "interval": 5
        }"#;
        let resp: DeviceAuthResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.device_code, "GmRhmhcxhwAzkoEqiMEg_DnyEysNkuNhszIySk9eS");
        assert_eq!(resp.user_code, "WDJB-MJHT");
        assert_eq!(resp.verification_uri, "https://idp.example.com/device");
        assert_eq!(resp.expires_in, 1800);
        assert_eq!(resp.interval, 5);
    }

    #[test]
    fn device_auth_response_default_interval() {
        let json = r#"{
            "device_code": "abc",
            "user_code": "XYZ",
            "verification_uri": "https://idp.example.com/device",
            "expires_in": 600
        }"#;
        let resp: DeviceAuthResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.interval, 5);
    }

    #[test]
    fn oauth_error_response_deserialization() {
        let json = r#"{"error": "authorization_pending", "error_description": "waiting for user"}"#;
        let resp: OAuthErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.error, "authorization_pending");
        assert_eq!(resp.error_description, "waiting for user");
    }

    #[test]
    fn server_discovery_response_deserialization() {
        let json =
            r#"{"idp_url": "https://keycloak.example.com/realms/hpc", "client_id": "pact-cli"}"#;
        let resp: ServerDiscoveryResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.idp_url, "https://keycloak.example.com/realms/hpc");
        assert_eq!(resp.client_id, "pact-cli");
    }

    #[test]
    fn manual_paste_url_uses_oob_redirect() {
        let url = build_auth_url(
            "https://idp.example.com/auth",
            "pact-cli",
            "urn:ietf:wg:oauth:2.0:oob",
            "test-state",
            None,
            "openid profile",
        );
        assert!(url.contains("redirect_uri=urn%3Aietf%3Awg%3Aoauth%3A2.0%3Aoob"));
    }

    #[test]
    fn urlencoding_encodes_special_chars() {
        assert_eq!(urlencoding::encode("hello world"), "hello%20world");
        assert_eq!(urlencoding::encode("a+b=c"), "a%2Bb%3Dc");
        assert_eq!(urlencoding::encode("safe-chars_here.ok~"), "safe-chars_here.ok~");
    }
}
