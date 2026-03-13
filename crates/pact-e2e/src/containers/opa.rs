//! OPA (Open Policy Agent) container for real Rego policy evaluation.
//!
//! Starts OPA in server mode with the pact authorization bundle.
//! Tests can push policies via the OPA REST API and evaluate them.

use testcontainers::core::wait::HttpWaitStrategy;
use testcontainers::core::{ContainerPort, WaitFor};
use testcontainers::Image;

/// OPA container port (REST API).
pub const OPA_PORT: ContainerPort = ContainerPort::Tcp(8181);

/// OPA container image for policy evaluation.
#[derive(Debug, Clone)]
pub struct Opa {
    tag: String,
}

impl Opa {
    /// Create a new OPA image with the given tag.
    pub fn new(tag: impl Into<String>) -> Self {
        Self { tag: tag.into() }
    }
}

impl Default for Opa {
    fn default() -> Self {
        Self { tag: "0.73.0-static".into() }
    }
}

impl Image for Opa {
    fn name(&self) -> &'static str {
        "openpolicyagent/opa"
    }

    fn tag(&self) -> &str {
        &self.tag
    }

    fn ready_conditions(&self) -> Vec<WaitFor> {
        vec![HttpWaitStrategy::new("/health").with_expected_status_code(200u16).into()]
    }

    fn expose_ports(&self) -> &[ContainerPort] {
        &[OPA_PORT]
    }

    fn cmd(&self) -> impl IntoIterator<Item = impl Into<std::borrow::Cow<'_, str>>> {
        ["run", "--server", "--addr", "0.0.0.0:8181", "--log-level", "error"]
    }
}
