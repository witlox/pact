//! Local OpenCHAMI REST client for BMC operations.
//!
//! Implements reboot (PowerCycle) and reimage (SetBootParams + PowerCycle)
//! via OpenCHAMI's SMD and BSS REST APIs.
//!
//! This is pact's own client — NOT imported from lattice, because pact's
//! management scope (BMC operations for config management) differs from
//! lattice's (workload scheduling).

use serde::{Deserialize, Serialize};

/// REST client for OpenCHAMI State Manager Daemon (SMD) and Boot Script Service (BSS).
#[derive(Debug, Clone)]
pub struct OpenChamiClient {
    smd_base_url: String,
    token: Option<String>,
    client: reqwest::Client,
}

#[derive(Debug, Serialize)]
struct PowerAction {
    #[serde(rename = "ResetType")]
    reset_type: String,
}

/// Component state returned by SMD.
#[derive(Debug, Deserialize)]
pub struct ComponentState {
    /// HSM component ID (xname).
    #[serde(rename = "ID")]
    pub id: String,
    /// Current component state (e.g., "Ready", "Off").
    pub state: String,
}

impl OpenChamiClient {
    /// Create a new client for the given SMD base URL.
    pub fn new(smd_base_url: &str, token: Option<&str>, timeout_secs: u64) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .build()
            .unwrap_or_default();
        Self {
            smd_base_url: smd_base_url.trim_end_matches('/').to_string(),
            token: token.map(str::to_string),
            client,
        }
    }

    fn auth_header(&self) -> Option<String> {
        self.token.as_ref().map(|t| format!("Bearer {t}"))
    }

    /// Power cycle (reboot) a node via Redfish.
    pub async fn reboot(&self, node_id: &str) -> Result<String, String> {
        let url = format!(
            "{}/hsm/v2/State/Components/{}/Actions/PowerCycle",
            self.smd_base_url, node_id
        );
        let body = PowerAction {
            reset_type: "ForceRestart".to_string(),
        };

        let mut req = self.client.post(&url).json(&body);
        if let Some(ref auth) = self.auth_header() {
            req = req.header("Authorization", auth);
        }

        let resp = req
            .send()
            .await
            .map_err(|e| format!("reboot request failed: {e}"))?;
        if resp.status().is_success() {
            Ok(format!("reboot initiated for {node_id}"))
        } else {
            Err(format!("reboot failed: HTTP {}", resp.status()))
        }
    }

    /// Re-image a node via BSS boot parameters + power cycle.
    ///
    /// The boot image selection is handled by OpenCHAMI BSS configuration,
    /// not by pact. Pact just triggers the reboot — BSS serves the new image
    /// on next boot.
    pub async fn reimage(&self, node_id: &str) -> Result<String, String> {
        self.reboot(node_id).await.map(|_| {
            format!("reimage initiated for {node_id} (power cycle + BSS re-provision)")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_client_trims_trailing_slash() {
        let client = OpenChamiClient::new("https://smd.example.com/", None, 30);
        assert_eq!(client.smd_base_url, "https://smd.example.com");
    }

    #[test]
    fn auth_header_with_token() {
        let client = OpenChamiClient::new("https://smd.example.com", Some("tok123"), 30);
        assert_eq!(client.auth_header(), Some("Bearer tok123".to_string()));
    }

    #[test]
    fn auth_header_without_token() {
        let client = OpenChamiClient::new("https://smd.example.com", None, 30);
        assert!(client.auth_header().is_none());
    }
}
