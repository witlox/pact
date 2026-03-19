//! OPA (Open Policy Agent) client for external policy evaluation.
//!
//! The `OpaClient` trait and `MockOpaClient` are always available.
//! `HttpOpaClient` requires the `opa` feature (uses reqwest).

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::PolicyError;

/// Input sent to OPA for policy evaluation.
#[derive(Debug, Clone, Serialize)]
pub struct OpaInput {
    pub identity: OpaIdentity,
    pub action: String,
    pub scope: OpaScope,
    pub command: Option<String>,
}

/// Identity portion of OPA input.
#[derive(Debug, Clone, Serialize)]
pub struct OpaIdentity {
    pub principal: String,
    pub role: String,
    pub principal_type: String,
}

/// Scope portion of OPA input.
#[derive(Debug, Clone, Serialize)]
pub struct OpaScope {
    pub vcluster: String,
}

/// Wrapper for OPA REST API request body.
#[derive(Debug, Clone, Serialize)]
pub struct OpaRequest {
    pub input: OpaInput,
}

/// Response from OPA allow endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct OpaAllowResponse {
    pub result: Option<bool>,
}

/// Response from OPA deny_reason endpoint.
#[derive(Debug, Clone, Deserialize)]
pub struct OpaDenyReasonResponse {
    pub result: Option<String>,
}

/// Decision returned by OPA evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpaDecision {
    Allow,
    Deny { reason: String },
}

/// Trait for OPA policy evaluation.
#[async_trait]
pub trait OpaClient: Send + Sync {
    /// Evaluate a policy input against OPA.
    async fn evaluate(&self, input: &OpaInput) -> Result<OpaDecision, PolicyError>;

    /// Check if the OPA service is healthy.
    fn health(&self) -> bool;
}

impl OpaInput {
    /// Convert a `PolicyRequest` into an `OpaInput`.
    pub fn from_request(request: &super::PolicyRequest) -> Self {
        let vcluster = match &request.scope {
            pact_common::types::Scope::VCluster(vc) | pact_common::types::Scope::Node(vc) => {
                vc.clone()
            }
            pact_common::types::Scope::Global => String::new(),
        };
        Self {
            identity: OpaIdentity {
                principal: request.identity.principal.clone(),
                role: request.identity.role.clone(),
                principal_type: format!("{:?}", request.identity.principal_type),
            },
            action: request.action.clone(),
            scope: OpaScope { vcluster },
            command: request.command.clone(),
        }
    }
}

// --- HttpOpaClient (feature-gated) ---

#[cfg(feature = "opa")]
pub struct HttpOpaClient {
    base_url: String,
    client: reqwest::Client,
}

#[cfg(feature = "opa")]
impl HttpOpaClient {
    /// Create a new HTTP OPA client with the given endpoint base URL.
    pub fn new(endpoint: &str) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .expect("failed to build reqwest client");
        Self { base_url: endpoint.trim_end_matches('/').to_string(), client }
    }

    /// Set a custom timeout duration.
    pub fn with_timeout(self, timeout: std::time::Duration) -> Self {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("failed to build reqwest client");
        Self { client, ..self }
    }
}

#[cfg(feature = "opa")]
#[async_trait]
impl OpaClient for HttpOpaClient {
    async fn evaluate(&self, input: &OpaInput) -> Result<OpaDecision, PolicyError> {
        let request_body = OpaRequest { input: input.clone() };

        // Query the allow endpoint
        let allow_url = format!("{}/v1/data/pact/authz/allow", self.base_url);
        let allow_resp = self
            .client
            .post(&allow_url)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| PolicyError::OpaError(format!("OPA request failed: {e}")))?;

        let allow_body: OpaAllowResponse = allow_resp
            .json()
            .await
            .map_err(|e| PolicyError::OpaError(format!("OPA response parse failed: {e}")))?;

        if allow_body.result.unwrap_or(false) {
            return Ok(OpaDecision::Allow);
        }

        // Query the deny_reason endpoint for a reason
        let deny_url = format!("{}/v1/data/pact/authz/deny_reason", self.base_url);
        let deny_resp =
            self.client.post(&deny_url).json(&request_body).send().await.map_err(|e| {
                PolicyError::OpaError(format!("OPA deny_reason request failed: {e}"))
            })?;

        let deny_body: OpaDenyReasonResponse = deny_resp.json().await.map_err(|e| {
            PolicyError::OpaError(format!("OPA deny_reason response parse failed: {e}"))
        })?;

        let reason = deny_body.result.unwrap_or_else(|| "denied by OPA policy".to_string());
        Ok(OpaDecision::Deny { reason })
    }

    fn health(&self) -> bool {
        true
    }
}

// --- MockOpaClient (always available) ---

/// Mock OPA client for testing. Not feature-gated.
pub struct MockOpaClient {
    pub healthy: bool,
    pub decision: OpaDecision,
}

impl MockOpaClient {
    /// Create a mock that always allows.
    pub fn allowing() -> Self {
        Self { healthy: true, decision: OpaDecision::Allow }
    }

    /// Create a mock that always denies with the given reason.
    pub fn denying(reason: &str) -> Self {
        Self { healthy: true, decision: OpaDecision::Deny { reason: reason.to_string() } }
    }

    /// Create a mock that is unhealthy (simulates OPA unavailability).
    pub fn unavailable() -> Self {
        Self { healthy: false, decision: OpaDecision::Allow }
    }
}

#[async_trait]
impl OpaClient for MockOpaClient {
    async fn evaluate(&self, _input: &OpaInput) -> Result<OpaDecision, PolicyError> {
        if !self.healthy {
            return Err(PolicyError::OpaError("OPA service unavailable".to_string()));
        }
        Ok(self.decision.clone())
    }

    fn health(&self) -> bool {
        self.healthy
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pact_common::types::{Identity, PrincipalType, Scope};

    #[tokio::test]
    async fn mock_allowing_returns_allow() {
        let client = MockOpaClient::allowing();
        let input = OpaInput {
            identity: OpaIdentity {
                principal: "admin@example.com".into(),
                role: "pact-platform-admin".into(),
                principal_type: "Human".into(),
            },
            action: "commit".into(),
            scope: OpaScope { vcluster: "ml".into() },
            command: None,
        };
        let result = client.evaluate(&input).await.unwrap();
        assert_eq!(result, OpaDecision::Allow);
    }

    #[tokio::test]
    async fn mock_denying_returns_deny_with_reason() {
        let client = MockOpaClient::denying("not authorized by OPA");
        let input = OpaInput {
            identity: OpaIdentity {
                principal: "user@example.com".into(),
                role: "pact-viewer-ml".into(),
                principal_type: "Human".into(),
            },
            action: "commit".into(),
            scope: OpaScope { vcluster: "ml".into() },
            command: None,
        };
        let result = client.evaluate(&input).await.unwrap();
        assert_eq!(result, OpaDecision::Deny { reason: "not authorized by OPA".to_string() });
    }

    #[tokio::test]
    async fn mock_unavailable_returns_error() {
        let client = MockOpaClient::unavailable();
        let input = OpaInput {
            identity: OpaIdentity {
                principal: "admin@example.com".into(),
                role: "pact-platform-admin".into(),
                principal_type: "Human".into(),
            },
            action: "commit".into(),
            scope: OpaScope { vcluster: "ml".into() },
            command: None,
        };
        let result = client.evaluate(&input).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, PolicyError::OpaError(_)));
    }

    #[test]
    fn opa_input_from_request_maps_correctly() {
        let request = super::super::PolicyRequest {
            identity: Identity {
                principal: "ops@example.com".into(),
                principal_type: PrincipalType::Human,
                role: "pact-ops-ml".into(),
            },
            scope: Scope::VCluster("ml-training".into()),
            action: "exec".into(),
            proposed_change: None,
            command: Some("nvidia-smi".into()),
        };

        let input = OpaInput::from_request(&request);
        assert_eq!(input.identity.principal, "ops@example.com");
        assert_eq!(input.identity.role, "pact-ops-ml");
        assert_eq!(input.identity.principal_type, "Human");
        assert_eq!(input.action, "exec");
        assert_eq!(input.scope.vcluster, "ml-training");
        assert_eq!(input.command, Some("nvidia-smi".into()));
    }
}
