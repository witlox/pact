//! CSM (Cray System Management) node management backend.
//!
//! Implements `NodeManagementBackend` for CSM deployments.
//! Reboot via CAPMC, reimage via BOS session. HSM path: `/smd/hsm/v2`.
//!
//! See specs/architecture/interfaces/node-management.md

use pact_common::node_mgmt::{NodeManagementBackend, NodeMgmtError};
use serde::{Deserialize, Serialize};

/// CSM backend — CAPMC for power control, BOS for boot orchestration.
#[derive(Debug, Clone)]
pub struct CsmBackend {
    base_url: String,
    token: Option<String>,
    client: reqwest::Client,
}

/// CAPMC power action request body.
#[derive(Debug, Serialize)]
struct CapmcPowerAction {
    reason: String,
    xnames: Vec<String>,
    force: bool,
}

/// BOS session request body.
#[derive(Debug, Serialize)]
struct BosSessionRequest {
    operation: String,
    limit: String,
}

/// BOS session response (minimal — we only need to know it was accepted).
#[derive(Debug, Deserialize)]
struct BosSessionResponse {
    #[serde(default)]
    name: String,
}

impl CsmBackend {
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

    /// Map HTTP error status to typed NodeMgmtError.
    fn map_http_error(status: u16, body: String) -> NodeMgmtError {
        if status == 401 || status == 403 {
            NodeMgmtError::AuthError(body)
        } else {
            NodeMgmtError::BackendError { status, body }
        }
    }
}

impl NodeManagementBackend for CsmBackend {
    fn reboot(
        &self,
        node_id: &str,
    ) -> impl std::future::Future<Output = Result<String, NodeMgmtError>> + Send {
        let node_id = node_id.to_string();
        let base_url = self.base_url.clone();
        let client = self.clone();

        async move {
            let url = format!("{base_url}/capmc/capmc/v1/xname_reinit");
            let body = CapmcPowerAction {
                reason: "pact reboot".to_string(),
                xnames: vec![node_id.clone()],
                force: false,
            };

            let resp = client
                .build_request(reqwest::Method::POST, &url)
                .json(&body)
                .send()
                .await
                .map_err(|e| NodeMgmtError::Unreachable(e.to_string()))?;

            if resp.status().is_success() {
                Ok(format!("reboot initiated for {node_id} via CAPMC"))
            } else {
                let status = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();
                Err(Self::map_http_error(status, body))
            }
        }
    }

    fn reimage(
        &self,
        node_id: &str,
    ) -> impl std::future::Future<Output = Result<String, NodeMgmtError>> + Send {
        let node_id = node_id.to_string();
        let base_url = self.base_url.clone();
        let client = self.clone();

        async move {
            // BOS reboot session — BOS uses the node's existing boot template (NM-I5).
            let url = format!("{base_url}/bos/v2/sessions");
            let body =
                BosSessionRequest { operation: "reboot".to_string(), limit: node_id.clone() };

            let resp = client
                .build_request(reqwest::Method::POST, &url)
                .json(&body)
                .send()
                .await
                .map_err(|e| NodeMgmtError::Unreachable(e.to_string()))?;

            if resp.status().is_success() {
                let session: BosSessionResponse =
                    resp.json().await.unwrap_or(BosSessionResponse { name: String::new() });
                let session_info = if session.name.is_empty() {
                    String::new()
                } else {
                    format!(" (session: {})", session.name)
                };
                Ok(format!("reimage initiated for {node_id} via BOS{session_info}"))
            } else {
                let status = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();
                // NM-ADV-3: BOS returns 404/400 when node has no boot template.
                if (status == 404 || status == 400)
                    && (body.contains("template") || body.contains("not found"))
                {
                    Err(NodeMgmtError::NoBootTemplate(node_id))
                } else {
                    Err(Self::map_http_error(status, body))
                }
            }
        }
    }

    fn hsm_path_prefix(&self) -> &'static str {
        "/smd/hsm/v2"
    }

    fn backend_name(&self) -> &'static str {
        "CSM"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_trims_trailing_slash() {
        let backend = CsmBackend::new("https://api.example.com/", None, 30);
        assert_eq!(backend.base_url, "https://api.example.com");
    }

    #[test]
    fn hsm_path_prefix() {
        let backend = CsmBackend::new("https://api.example.com", None, 30);
        assert_eq!(backend.hsm_path_prefix(), "/smd/hsm/v2");
    }

    #[test]
    fn backend_name() {
        let backend = CsmBackend::new("https://api.example.com", None, 30);
        assert_eq!(backend.backend_name(), "CSM");
    }

    #[test]
    fn capmc_body_serialization() {
        let body = CapmcPowerAction {
            reason: "pact reboot".to_string(),
            xnames: vec!["x1000c0s0b0n0".to_string()],
            force: false,
        };
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["reason"], "pact reboot");
        assert_eq!(json["xnames"][0], "x1000c0s0b0n0");
        assert_eq!(json["force"], false);
    }

    #[test]
    fn bos_session_body_serialization() {
        let body = BosSessionRequest {
            operation: "reboot".to_string(),
            limit: "x1000c0s0b0n0".to_string(),
        };
        let json = serde_json::to_value(&body).unwrap();
        assert_eq!(json["operation"], "reboot");
        assert_eq!(json["limit"], "x1000c0s0b0n0");
    }
}
