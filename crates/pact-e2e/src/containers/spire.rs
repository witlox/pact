//! SPIRE server + agent containers for e2e testing.
//!
//! Starts a SPIRE server and agent pair. The agent provides the
//! Workload API socket that `SpireProvider` connects to.
//!
//! Uses the official SPIRE container images from ghcr.io/spiffe.

use testcontainers::core::{ContainerPort, WaitFor};
use testcontainers::Image;

/// SPIRE server port (gRPC).
pub const SPIRE_SERVER_PORT: ContainerPort = ContainerPort::Tcp(8081);

/// SPIRE server container image.
#[derive(Debug, Clone)]
pub struct SpireServer {
    tag: String,
}

impl SpireServer {
    pub fn new(tag: impl Into<String>) -> Self {
        Self { tag: tag.into() }
    }
}

impl Default for SpireServer {
    fn default() -> Self {
        Self {
            tag: "1.12.0".into(),
        }
    }
}

impl Image for SpireServer {
    fn name(&self) -> &'static str {
        "ghcr.io/spiffe/spire-server"
    }

    fn tag(&self) -> &str {
        &self.tag
    }

    fn ready_conditions(&self) -> Vec<WaitFor> {
        vec![WaitFor::message_on_stderr("Starting gRPC server")]
    }

    fn expose_ports(&self) -> &[ContainerPort] {
        &[SPIRE_SERVER_PORT]
    }

    fn cmd(&self) -> impl IntoIterator<Item = impl Into<std::borrow::Cow<'_, str>>> {
        [
            "-config",
            "/opt/spire/conf/server/server.conf",
        ]
    }
}

/// SPIRE agent port (Workload API — normally a unix socket, but
/// for testcontainers we use TCP for cross-container access).
pub const SPIRE_AGENT_PORT: ContainerPort = ContainerPort::Tcp(8082);

/// SPIRE agent container image.
#[derive(Debug, Clone)]
pub struct SpireAgent {
    tag: String,
}

impl SpireAgent {
    pub fn new(tag: impl Into<String>) -> Self {
        Self { tag: tag.into() }
    }
}

impl Default for SpireAgent {
    fn default() -> Self {
        Self {
            tag: "1.12.0".into(),
        }
    }
}

impl Image for SpireAgent {
    fn name(&self) -> &'static str {
        "ghcr.io/spiffe/spire-agent"
    }

    fn tag(&self) -> &str {
        &self.tag
    }

    fn ready_conditions(&self) -> Vec<WaitFor> {
        vec![WaitFor::message_on_stderr("Starting Workload API")]
    }

    fn expose_ports(&self) -> &[ContainerPort] {
        &[SPIRE_AGENT_PORT]
    }

    fn cmd(&self) -> impl IntoIterator<Item = impl Into<std::borrow::Cow<'_, str>>> {
        [
            "-config",
            "/opt/spire/conf/agent/agent.conf",
        ]
    }
}
