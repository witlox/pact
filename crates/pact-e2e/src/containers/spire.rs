//! SPIRE server + agent containers for e2e testing.
//!
//! Uses join_token attestation for simplicity. The flow:
//! 1. Start SPIRE server with server.conf
//! 2. Create a join token via `spire-server token generate`
//! 3. Create a registration entry for the workload
//! 4. Start SPIRE agent with the join token
//! 5. Agent provides Workload API socket at /tmp/spire-agent/agent.sock

use testcontainers::core::{ContainerPort, WaitFor};
use testcontainers::Image;

/// SPIRE server port (gRPC for agent connections).
pub const SPIRE_SERVER_PORT: ContainerPort = ContainerPort::Tcp(8081);

/// SPIRE server container.
#[derive(Debug, Clone)]
pub struct SpireServer {
    tag: String,
}

impl SpireServer {
    #[must_use]
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
        ["-config", "/opt/spire/conf/server/server.conf"]
    }
}

/// SPIRE agent Workload API socket port.
/// In testcontainer mode we expose it as TCP for cross-container access.
pub const SPIRE_AGENT_PORT: ContainerPort = ContainerPort::Tcp(8082);

/// SPIRE agent container.
#[derive(Debug, Clone)]
pub struct SpireAgent {
    tag: String,
}

impl SpireAgent {
    #[must_use]
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
        ["-config", "/opt/spire/conf/agent/agent.conf"]
    }
}

/// Helper to generate a SPIRE join token by exec'ing into the server container.
///
/// Returns the token string that the agent uses for attestation.
pub async fn generate_join_token(
    container: &testcontainers::ContainerAsync<SpireServer>,
    spiffe_id: &str,
) -> anyhow::Result<String> {
    // Execute: spire-server token generate -spiffeID <id> -ttl 300
    let mut output = container
        .exec(testcontainers::core::ExecCommand::new([
            "/opt/spire/bin/spire-server",
            "token",
            "generate",
            "-spiffeID",
            spiffe_id,
            "-ttl",
            "300",
            "-output",
            "json",
        ]))
        .await?;

    let stdout = output.stdout_to_vec().await?;
    let stdout_str = String::from_utf8_lossy(&stdout);

    // Parse JSON output to extract token
    let parsed: serde_json::Value = serde_json::from_str(&stdout_str)
        .map_err(|e| anyhow::anyhow!("failed to parse join token output: {e}\nstdout: {stdout_str}"))?;

    parsed["value"]
        .as_str()
        .map(String::from)
        .ok_or_else(|| anyhow::anyhow!("no token value in output: {stdout_str}"))
}

/// Helper to create a registration entry for a workload.
pub async fn create_registration_entry(
    container: &testcontainers::ContainerAsync<SpireServer>,
    spiffe_id: &str,
    parent_id: &str,
    selector: &str,
) -> anyhow::Result<()> {
    let mut output = container
        .exec(testcontainers::core::ExecCommand::new([
            "/opt/spire/bin/spire-server",
            "entry",
            "create",
            "-spiffeID",
            spiffe_id,
            "-parentID",
            parent_id,
            "-selector",
            selector,
        ]))
        .await?;

    let stderr = output.stderr_to_vec().await?;
    let stderr_str = String::from_utf8_lossy(&stderr);

    if stderr_str.contains("Error") {
        anyhow::bail!("registration entry creation failed: {stderr_str}");
    }

    Ok(())
}
