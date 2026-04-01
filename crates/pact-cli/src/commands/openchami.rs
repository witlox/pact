//! OpenCHAMI node management backend.
//!
//! Implements `NodeManagementBackend` for OpenCHAMI deployments.
//! Reboot/reimage via SMD Redfish PowerCycle. HSM path: `/hsm/v2`.
//!
//! See specs/architecture/interfaces/node-management.md

use pact_common::node_mgmt::{NodeManagementBackend, NodeMgmtError};
use serde::Serialize;

/// OpenCHAMI backend — SMD Redfish for power, BSS for boot (implicit on reboot).
#[derive(Debug, Clone)]
pub struct OpenChamiBackend {
    base_url: String,
    token: Option<String>,
    client: reqwest::Client,
}

#[derive(Debug, Serialize)]
struct PowerAction {
    #[serde(rename = "ResetType")]
    reset_type: String,
}

impl OpenChamiBackend {
    pub fn new(base_url: &str, token: Option<&str>, timeout_secs: u64) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .build()
            .unwrap_or_default();
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            token: token.map(str::to_string),
            client,
        }
    }

    fn build_request(&self, method: reqwest::Method, url: &str) -> reqwest::RequestBuilder {
        let mut req = self.client.request(method, url);
        if let Some(ref token) = self.token {
            req = req.bearer_auth(token);
        }
        req
    }

    /// Power cycle via Redfish.
    async fn power_cycle(&self, node_id: &str) -> Result<String, NodeMgmtError> {
        let url =
            format!("{}/hsm/v2/State/Components/{}/Actions/PowerCycle", self.base_url, node_id);
        let body = PowerAction { reset_type: "ForceRestart".to_string() };

        let resp = self
            .build_request(reqwest::Method::POST, &url)
            .json(&body)
            .send()
            .await
            .map_err(|e| NodeMgmtError::Unreachable(e.to_string()))?;

        if resp.status().is_success() {
            Ok(format!("power cycle initiated for {node_id}"))
        } else {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            if status == 401 || status == 403 {
                Err(NodeMgmtError::AuthError(body))
            } else {
                Err(NodeMgmtError::BackendError { status, body })
            }
        }
    }
}

impl NodeManagementBackend for OpenChamiBackend {
    fn reboot(
        &self,
        node_id: &str,
    ) -> impl std::future::Future<Output = Result<String, NodeMgmtError>> + Send {
        let node_id = node_id.to_string();
        async move { self.power_cycle(&node_id).await }
    }

    fn reimage(
        &self,
        node_id: &str,
    ) -> impl std::future::Future<Output = Result<String, NodeMgmtError>> + Send {
        // OpenCHAMI: reimage = reboot. BSS serves the new image on next boot (NM-I5).
        let node_id = node_id.to_string();
        async move {
            self.power_cycle(&node_id).await.map(|_| {
                format!("reimage initiated for {node_id} (power cycle + BSS re-provision)")
            })
        }
    }

    fn hsm_path_prefix(&self) -> &'static str {
        "/hsm/v2"
    }

    fn backend_name(&self) -> &'static str {
        "OpenCHAMI"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_trims_trailing_slash() {
        let backend = OpenChamiBackend::new("https://smd.example.com/", None, 30);
        assert_eq!(backend.base_url, "https://smd.example.com");
    }

    #[test]
    fn hsm_path_prefix() {
        let backend = OpenChamiBackend::new("https://smd.example.com", None, 30);
        assert_eq!(backend.hsm_path_prefix(), "/hsm/v2");
    }

    #[test]
    fn backend_name() {
        let backend = OpenChamiBackend::new("https://smd.example.com", None, 30);
        assert_eq!(backend.backend_name(), "OpenCHAMI");
    }
}
