//! Command execution — gRPC client calls to journal services.
//!
//! Each function connects to the journal, executes the request,
//! and returns the formatted result.

use tonic::transport::{Certificate, Channel, ClientTlsConfig, Identity};
use tracing::debug;

use pact_common::proto::config::{
    scope::Scope as ProtoScope, ConfigEntry as ProtoConfigEntry, Identity as ProtoIdentity,
    Scope as ProtoScopeMsg,
};
use pact_common::proto::journal::config_service_client::ConfigServiceClient;
use pact_common::proto::journal::{AppendEntryRequest, GetNodeStateRequest, ListEntriesRequest};

use super::config::CliConfig;
use super::promote;

/// Resolve identity (principal + role) from a JWT token.
///
/// Decodes the JWT payload without signature verification (the journal validates it).
/// Falls back to defaults if decoding fails.
pub fn resolve_identity_from_token(token: &str) -> (String, String) {
    use jsonwebtoken::dangerous::insecure_decode;

    #[derive(serde::Deserialize)]
    struct Claims {
        sub: Option<String>,
        pact_role: Option<String>,
    }

    match insecure_decode::<Claims>(token) {
        Ok(data) => {
            let principal = data.claims.sub.unwrap_or_else(|| "cli-user".to_string());
            let role = data.claims.pact_role.unwrap_or_else(|| "pact-platform-admin".to_string());
            (principal, role)
        }
        Err(_) => ("cli-user".to_string(), "pact-platform-admin".to_string()),
    }
}

/// TLS configuration for CLI connections.
#[derive(Debug, Clone, Default)]
pub struct TlsOptions {
    /// Path to CA certificate PEM file.
    pub ca_cert: Option<std::path::PathBuf>,
    /// Path to client certificate PEM file.
    pub client_cert: Option<std::path::PathBuf>,
    /// Path to client key PEM file.
    pub client_key: Option<std::path::PathBuf>,
}

impl TlsOptions {
    /// Build a `ClientTlsConfig` from these options.
    pub fn to_tls_config(&self) -> anyhow::Result<ClientTlsConfig> {
        let mut tls = ClientTlsConfig::new();

        if let Some(ca_path) = &self.ca_cert {
            let ca_pem = std::fs::read_to_string(ca_path)
                .map_err(|e| anyhow::anyhow!("cannot read CA cert {}: {e}", ca_path.display()))?;
            tls = tls.ca_certificate(Certificate::from_pem(ca_pem));
        }

        if let (Some(cert_path), Some(key_path)) = (&self.client_cert, &self.client_key) {
            let cert_pem = std::fs::read_to_string(cert_path).map_err(|e| {
                anyhow::anyhow!("cannot read client cert {}: {e}", cert_path.display())
            })?;
            let key_pem = std::fs::read_to_string(key_path).map_err(|e| {
                anyhow::anyhow!("cannot read client key {}: {e}", key_path.display())
            })?;
            tls = tls.identity(Identity::from_pem(cert_pem, key_pem));
        }

        Ok(tls)
    }
}

/// Create a gRPC channel to the journal endpoint.
///
/// If the endpoint starts with `https`, TLS is configured automatically.
/// Provide `tls_options` for mTLS (client certificate authentication).
pub async fn connect(config: &CliConfig) -> anyhow::Result<Channel> {
    connect_with_tls(config, None).await
}

/// Create a gRPC channel with optional TLS configuration.
pub async fn connect_with_tls(
    config: &CliConfig,
    tls_options: Option<&TlsOptions>,
) -> anyhow::Result<Channel> {
    let uri = if config.endpoint.starts_with("http") {
        config.endpoint.clone()
    } else {
        format!("http://{}", config.endpoint)
    };

    let mut channel_builder = Channel::from_shared(uri.clone())
        .map_err(|e| anyhow::anyhow!("invalid endpoint {uri}: {e}"))?
        .timeout(std::time::Duration::from_secs(u64::from(config.timeout_seconds)));

    // Configure TLS if the endpoint is https or explicit TLS options are provided
    if uri.starts_with("https") || tls_options.is_some() {
        let tls_config = if let Some(opts) = tls_options {
            opts.to_tls_config()?
        } else {
            ClientTlsConfig::new()
        };
        channel_builder = channel_builder
            .tls_config(tls_config)
            .map_err(|e| anyhow::anyhow!("TLS config error for {uri}: {e}"))?;
    }

    let channel = channel_builder
        .connect()
        .await
        .map_err(|e| anyhow::anyhow!("cannot connect to journal at {uri}: {e}"))?;

    debug!(endpoint = %uri, "Connected to journal");
    Ok(channel)
}

/// Create a gRPC channel with Bearer token injected into all requests (P1).
///
/// Uses a tonic interceptor so ALL RPCs on this channel automatically
/// carry the auth header. This prevents the error-prone pattern of
/// manually injecting tokens per-request.
pub async fn connect_authenticated(
    config: &CliConfig,
    token: String,
) -> anyhow::Result<AuthenticatedChannel> {
    let channel = connect(config).await?;
    Ok(AuthenticatedChannel { channel, token })
}

/// A gRPC channel wrapper that injects Bearer token on every request.
#[derive(Clone)]
pub struct AuthenticatedChannel {
    channel: Channel,
    token: String,
}

impl AuthenticatedChannel {
    /// Create from a raw channel + token.
    pub fn new(channel: Channel, token: String) -> Self {
        Self { channel, token }
    }

    /// Create a `ConfigServiceClient` with auth interceptor.
    pub fn config_client(&self) -> AuthConfigClient {
        ConfigServiceClient::with_interceptor(
            self.channel.clone(),
            AuthInterceptor::new(self.token.clone()),
        )
    }

    /// Create a `PolicyServiceClient` with auth interceptor.
    pub fn policy_client(
        &self,
    ) -> pact_common::proto::policy::policy_service_client::PolicyServiceClient<
        tonic::service::interceptor::InterceptedService<Channel, AuthInterceptor>,
    > {
        pact_common::proto::policy::policy_service_client::PolicyServiceClient::with_interceptor(
            self.channel.clone(),
            AuthInterceptor::new(self.token.clone()),
        )
    }

    /// Create a `BootConfigServiceClient` with auth interceptor.
    pub fn boot_config_client(
        &self,
    ) -> pact_common::proto::stream::boot_config_service_client::BootConfigServiceClient<
        tonic::service::interceptor::InterceptedService<Channel, AuthInterceptor>,
    > {
        pact_common::proto::stream::boot_config_service_client::BootConfigServiceClient::with_interceptor(
            self.channel.clone(),
            AuthInterceptor::new(self.token.clone()),
        )
    }

    /// Create an `EnrollmentServiceClient` with auth interceptor.
    pub fn enrollment_client(
        &self,
    ) -> pact_common::proto::enrollment::enrollment_service_client::EnrollmentServiceClient<
        tonic::service::interceptor::InterceptedService<Channel, AuthInterceptor>,
    > {
        pact_common::proto::enrollment::enrollment_service_client::EnrollmentServiceClient::with_interceptor(
            self.channel.clone(),
            AuthInterceptor::new(self.token.clone()),
        )
    }

    /// Create an authenticated `ShellServiceClient` for agent calls.
    /// Agent is a different endpoint but uses the same token.
    pub fn shell_client_for(
        channel: Channel,
        token: &str,
    ) -> pact_common::proto::shell::shell_service_client::ShellServiceClient<
        tonic::service::interceptor::InterceptedService<Channel, AuthInterceptor>,
    > {
        pact_common::proto::shell::shell_service_client::ShellServiceClient::with_interceptor(
            channel,
            AuthInterceptor::new(token.to_string()),
        )
    }

    /// Get the raw channel.
    pub fn channel(&self) -> &Channel {
        &self.channel
    }

    /// Get the token.
    pub fn token(&self) -> &str {
        &self.token
    }
}

/// gRPC interceptor that injects Bearer token into request metadata.
#[derive(Clone)]
pub struct AuthInterceptor {
    pub token: String,
}

impl AuthInterceptor {
    pub fn new(token: String) -> Self {
        Self { token }
    }
}

impl tonic::service::Interceptor for AuthInterceptor {
    fn call(&mut self, mut req: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
        if !self.token.is_empty() {
            req.metadata_mut().insert(
                "authorization",
                format!("Bearer {}", self.token)
                    .parse()
                    .map_err(|_| tonic::Status::internal("invalid token format"))?,
            );
        }
        Ok(req)
    }
}

/// Type alias for the authenticated config client.
pub type AuthConfigClient =
    ConfigServiceClient<tonic::service::interceptor::InterceptedService<Channel, AuthInterceptor>>;

/// Execute `pact status` — query node state from journal.
pub async fn status(client: &mut AuthConfigClient, node_id: &str) -> anyhow::Result<String> {
    let resp = client
        .get_node_state(tonic::Request::new(GetNodeStateRequest { node_id: node_id.to_string() }))
        .await
        .map_err(|e| anyhow::anyhow!("status query failed: {e}"))?;

    let ns = resp.into_inner();
    Ok(format!("Node: {}  State: {}", ns.node_id, ns.config_state))
}

/// Execute `pact log` — list recent config entries from journal.
pub async fn log(
    client: &mut AuthConfigClient,
    limit: u32,
    scope: Option<&str>,
) -> anyhow::Result<String> {
    let resp = client
        .list_entries(tonic::Request::new(ListEntriesRequest {
            scope: scope.map(parse_scope_filter),
            from_sequence: None,
            to_sequence: None,
            limit: Some(limit),
        }))
        .await
        .map_err(|e| anyhow::anyhow!("log query failed: {e}"))?;

    let mut stream = resp.into_inner();
    let mut entries = Vec::new();
    while let Some(entry) = tokio_stream::StreamExt::next(&mut stream).await {
        match entry {
            Ok(e) => entries.push(format_proto_entry(&e)),
            Err(e) => return Err(anyhow::anyhow!("log stream error: {e}")),
        }
    }

    if entries.is_empty() {
        Ok("No entries found.".to_string())
    } else {
        Ok(entries.join("\n"))
    }
}

/// Execute `pact commit` — append a Commit entry through Raft.
pub async fn commit(
    client: &mut AuthConfigClient,
    message: &str,
    vcluster: &str,
    principal: &str,
    role: &str,
) -> anyhow::Result<String> {
    let entry = ProtoConfigEntry {
        sequence: 0, // assigned by journal
        timestamp: None,
        entry_type: 1, // Commit
        scope: Some(ProtoScopeMsg { scope: Some(ProtoScope::VclusterId(vcluster.to_string())) }),
        author: Some(ProtoIdentity {
            principal: principal.to_string(),
            principal_type: "Human".to_string(),
            role: role.to_string(),
        }),
        parent: None,
        state_delta: None,
        policy_ref: message.to_string(), // use message as policy_ref for now
        ttl: None,
        emergency_reason: None,
    };

    let resp = client
        .append_entry(tonic::Request::new(AppendEntryRequest { entry: Some(entry) }))
        .await
        .map_err(|e| anyhow::anyhow!("commit failed: {e}"))?;

    let seq = resp.into_inner().sequence;
    Ok(format!("Committed (seq:{seq}) on vCluster: {vcluster}"))
}

/// Execute `pact rollback` — append a Rollback entry referencing a target sequence.
pub async fn rollback(
    client: &mut AuthConfigClient,
    target_seq: u64,
    vcluster: &str,
    principal: &str,
    role: &str,
) -> anyhow::Result<String> {
    let entry = ProtoConfigEntry {
        sequence: 0,
        timestamp: None,
        entry_type: 2, // Rollback
        scope: Some(ProtoScopeMsg { scope: Some(ProtoScope::VclusterId(vcluster.to_string())) }),
        author: Some(ProtoIdentity {
            principal: principal.to_string(),
            principal_type: "Human".to_string(),
            role: role.to_string(),
        }),
        parent: Some(target_seq),
        state_delta: None,
        policy_ref: String::new(),
        ttl: None,
        emergency_reason: None,
    };

    let resp = client
        .append_entry(tonic::Request::new(AppendEntryRequest { entry: Some(entry) }))
        .await
        .map_err(|e| anyhow::anyhow!("rollback failed: {e}"))?;

    let seq = resp.into_inner().sequence;
    Ok(format!("Rolled back to seq:{target_seq} (new seq:{seq})"))
}

/// Connect to an agent's shell gRPC endpoint.
pub async fn connect_agent(agent_addr: &str) -> anyhow::Result<Channel> {
    let uri = if agent_addr.starts_with("http") {
        agent_addr.to_string()
    } else {
        format!("http://{agent_addr}")
    };

    Channel::from_shared(uri.clone())
        .map_err(|e| anyhow::anyhow!("invalid agent endpoint {uri}: {e}"))?
        .connect()
        .await
        .map_err(|e| anyhow::anyhow!("cannot connect to agent at {uri}: {e}"))
}

/// Default agent gRPC port.
const AGENT_DEFAULT_PORT: u16 = 9445;

/// Resolve the agent address for a given node ID.
///
/// Queries the journal `ConfigService.GetNodeState` to verify the node exists,
/// then returns `http://{node_id}:9445` (DNS-based discovery).
/// Falls back to `http://127.0.0.1:9445` for the "local" node ID.
pub async fn resolve_agent_address(
    node_id: &str,
    auth_channel: &AuthenticatedChannel,
) -> anyhow::Result<String> {
    if node_id == "local" || node_id == "localhost" {
        return Ok(format!("http://127.0.0.1:{AGENT_DEFAULT_PORT}"));
    }

    // Verify the node exists by querying journal
    let mut client = auth_channel.config_client();
    let _resp = client
        .get_node_state(tonic::Request::new(GetNodeStateRequest { node_id: node_id.to_string() }))
        .await
        .map_err(|e| anyhow::anyhow!("cannot resolve node '{node_id}': {e}"))?;

    Ok(format!("http://{node_id}:{AGENT_DEFAULT_PORT}"))
}

/// Execute `pact exec` — run a command on a remote node via ShellService.
pub async fn exec_remote(
    channel: Channel,
    token: &str,
    command: &str,
    args: &[String],
) -> anyhow::Result<String> {
    use pact_common::proto::shell::{exec_output, ExecRequest};

    let mut client = AuthenticatedChannel::shell_client_for(channel, token);

    let request =
        tonic::Request::new(ExecRequest { command: command.to_string(), args: args.to_vec() });

    let resp = client.exec(request).await.map_err(|e| anyhow::anyhow!("exec failed: {e}"))?;

    let mut stream = resp.into_inner();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut exit_code = 0i32;

    while let Some(output) = tokio_stream::StreamExt::next(&mut stream).await {
        match output {
            Ok(o) => match o.output {
                Some(exec_output::Output::Stdout(data)) => stdout.extend_from_slice(&data),
                Some(exec_output::Output::Stderr(data)) => stderr.extend_from_slice(&data),
                Some(exec_output::Output::ExitCode(code)) => exit_code = code,
                Some(exec_output::Output::Error(e)) => return Err(anyhow::anyhow!("{e}")),
                None => {}
            },
            Err(e) => return Err(anyhow::anyhow!("exec stream error: {e}")),
        }
    }

    let mut output = String::new();
    if !stdout.is_empty() {
        output.push_str(&String::from_utf8_lossy(&stdout));
    }
    if !stderr.is_empty() {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(&String::from_utf8_lossy(&stderr));
    }
    if exit_code != 0 {
        output.push_str(&format!("\n(exit code: {exit_code})"));
    }

    Ok(output)
}

/// Execute `pact service status` — list commands via ShellService.
pub async fn list_agent_commands(channel: Channel, token: &str) -> anyhow::Result<String> {
    use pact_common::proto::shell::ListCommandsRequest;

    let mut client = AuthenticatedChannel::shell_client_for(channel, token);
    let resp = client
        .list_commands(tonic::Request::new(ListCommandsRequest {}))
        .await
        .map_err(|e| anyhow::anyhow!("list commands failed: {e}"))?;

    let commands = resp.into_inner().commands;
    if commands.is_empty() {
        return Ok("No commands available.".to_string());
    }

    let mut output = format!("{:<24} {:<6} {}\n", "COMMAND", "STATE", "DESCRIPTION");
    for cmd in &commands {
        let state = if cmd.state_changing { "yes" } else { "no" };
        output.push_str(&format!("{:<24} {:<6} {}\n", cmd.command, state, cmd.description));
    }
    Ok(output)
}

/// Execute `pact apply` — parse a TOML spec and submit config entries.
pub async fn apply(
    client: &mut AuthConfigClient,
    spec_path: &str,
    principal: &str,
    role: &str,
) -> anyhow::Result<String> {
    use super::apply::{format_spec_summary, load_spec, spec_to_delta};
    use pact_journal::service::state_delta_to_proto;

    let spec = load_spec(std::path::Path::new(spec_path))?;

    if spec.vcluster.is_empty() {
        return Ok("No changes in spec file.".to_string());
    }

    let summary = format_spec_summary(&spec);
    let mut results = Vec::new();

    for (vc_name, vc_spec) in &spec.vcluster {
        let delta = spec_to_delta(vc_spec);
        let proto_delta = state_delta_to_proto(&delta);

        let entry = ProtoConfigEntry {
            sequence: 0,
            timestamp: None,
            entry_type: 1, // Commit
            scope: Some(ProtoScopeMsg { scope: Some(ProtoScope::VclusterId(vc_name.clone())) }),
            author: Some(ProtoIdentity {
                principal: principal.to_string(),
                principal_type: "Human".to_string(),
                role: role.to_string(),
            }),
            parent: None,
            state_delta: Some(proto_delta),
            policy_ref: format!("apply:{spec_path}"),
            ttl: None,
            emergency_reason: None,
        };

        let resp = client
            .append_entry(tonic::Request::new(AppendEntryRequest { entry: Some(entry) }))
            .await
            .map_err(|e| anyhow::anyhow!("apply failed for {vc_name}: {e}"))?;

        let seq = resp.into_inner().sequence;
        results.push(format!("Applied to {vc_name} (seq:{seq})"));
    }

    Ok(format!("{summary}\n\n{}", results.join("\n")))
}

/// Convert proto StateDelta back to domain StateDelta.
fn proto_to_state_delta(
    proto: &pact_common::proto::config::StateDelta,
) -> pact_common::types::StateDelta {
    use pact_common::types::{DeltaAction, DeltaItem};

    fn proto_action(a: i32) -> DeltaAction {
        match a {
            1 => DeltaAction::Add,
            2 => DeltaAction::Remove,
            _ => DeltaAction::Modify, // 3 or unknown → Modify
        }
    }

    pact_common::types::StateDelta {
        kernel: proto
            .kernel
            .iter()
            .map(|k| DeltaItem {
                action: proto_action(k.action),
                key: k.key.clone(),
                value: k.declared_value.clone(),
                previous: k.actual_value.clone(),
            })
            .collect(),
        services: proto
            .services
            .iter()
            .map(|s| DeltaItem {
                action: proto_action(s.action),
                key: s.name.clone(),
                value: s.declared_state.clone(),
                previous: s.actual_state.clone(),
            })
            .collect(),
        mounts: proto
            .mounts
            .iter()
            .map(|m| DeltaItem {
                action: proto_action(m.action),
                key: m.path.clone(),
                value: None,
                previous: None,
            })
            .collect(),
        files: proto
            .files
            .iter()
            .map(|f| DeltaItem {
                action: proto_action(f.action),
                key: f.path.clone(),
                value: f.content_hash.clone(),
                previous: f.owner.clone(),
            })
            .collect(),
        network: proto
            .network
            .iter()
            .map(|n| DeltaItem {
                action: proto_action(n.action),
                key: n.interface.clone(),
                value: n.detail.clone(),
                previous: None,
            })
            .collect(),
        packages: proto
            .packages
            .iter()
            .map(|p| DeltaItem {
                action: proto_action(p.action),
                key: p.name.clone(),
                value: p.version.clone(),
                previous: None,
            })
            .collect(),
        gpu: proto
            .gpu
            .iter()
            .map(|g| DeltaItem {
                action: proto_action(g.action),
                key: g.gpu_index.to_string(),
                value: g.detail.clone(),
                previous: None,
            })
            .collect(),
    }
}

/// Execute `pact promote` — export committed node deltas as overlay TOML.
pub async fn promote_node(
    client: &mut AuthConfigClient,
    node_id: &str,
    dry_run: bool,
) -> anyhow::Result<String> {
    let resp = client
        .list_entries(tonic::Request::new(ListEntriesRequest {
            scope: Some(ProtoScopeMsg { scope: Some(ProtoScope::NodeId(node_id.to_string())) }),
            from_sequence: None,
            to_sequence: None,
            limit: None,
        }))
        .await
        .map_err(|e| anyhow::anyhow!("promote query failed: {e}"))?;

    let mut stream = resp.into_inner();
    let mut deltas = Vec::new();

    while let Some(entry) = tokio_stream::StreamExt::next(&mut stream).await {
        match entry {
            Ok(e) => {
                // Filter for Commit entries (entry_type 1) with a state_delta
                if e.entry_type == 1 {
                    if let Some(ref proto_delta) = e.state_delta {
                        deltas.push(proto_to_state_delta(proto_delta));
                    }
                }
            }
            Err(e) => return Err(anyhow::anyhow!("promote stream error: {e}")),
        }
    }

    if deltas.is_empty() {
        return Ok(format!("No committed deltas found for node {node_id}."));
    }

    let count = deltas.len();

    if dry_run {
        // Summarize which sections would be exported
        let mut sections = Vec::new();
        for d in &deltas {
            if !d.kernel.is_empty() {
                sections.push("kernel");
            }
            if !d.services.is_empty() {
                sections.push("services");
            }
            if !d.mounts.is_empty() {
                sections.push("mounts");
            }
            if !d.files.is_empty() {
                sections.push("files");
            }
            if !d.network.is_empty() {
                sections.push("network");
            }
            if !d.packages.is_empty() {
                sections.push("packages");
            }
            if !d.gpu.is_empty() {
                sections.push("gpu");
            }
        }
        sections.sort_unstable();
        sections.dedup();
        Ok(format!(
            "DRY RUN — would export {count} delta(s) from node {node_id}\nSections: {}",
            if sections.is_empty() { "(none)".to_string() } else { sections.join(", ") }
        ))
    } else {
        let toml = promote::export_deltas_as_toml(&deltas);
        Ok(format!("Exported {count} delta(s) from node {node_id}\n\n{toml}"))
    }
}

/// Execute `pact emergency start` — append EmergencyStart entry through Raft.
pub async fn emergency_start(
    client: &mut AuthConfigClient,
    reason: &str,
    vcluster: &str,
    principal: &str,
    role: &str,
) -> anyhow::Result<String> {
    let entry = ProtoConfigEntry {
        sequence: 0,
        timestamp: None,
        entry_type: 8, // EmergencyStart
        scope: Some(ProtoScopeMsg { scope: Some(ProtoScope::VclusterId(vcluster.to_string())) }),
        author: Some(ProtoIdentity {
            principal: principal.to_string(),
            principal_type: "Human".to_string(),
            role: role.to_string(),
        }),
        parent: None,
        state_delta: None,
        policy_ref: String::new(),
        ttl: None,
        emergency_reason: Some(reason.to_string()),
    };

    let resp = client
        .append_entry(tonic::Request::new(AppendEntryRequest { entry: Some(entry) }))
        .await
        .map_err(|e| anyhow::anyhow!("emergency start failed: {e}"))?;

    let seq = resp.into_inner().sequence;
    Ok(format!("Emergency mode ACTIVE (seq:{seq}) on vCluster: {vcluster}\nReason: {reason}"))
}

/// Execute `pact emergency end` — append EmergencyEnd entry through Raft.
pub async fn emergency_end(
    client: &mut AuthConfigClient,
    vcluster: &str,
    principal: &str,
    role: &str,
) -> anyhow::Result<String> {
    let entry = ProtoConfigEntry {
        sequence: 0,
        timestamp: None,
        entry_type: 9, // EmergencyEnd
        scope: Some(ProtoScopeMsg { scope: Some(ProtoScope::VclusterId(vcluster.to_string())) }),
        author: Some(ProtoIdentity {
            principal: principal.to_string(),
            principal_type: "Human".to_string(),
            role: role.to_string(),
        }),
        parent: None,
        state_delta: None,
        policy_ref: String::new(),
        ttl: None,
        emergency_reason: None,
    };

    let resp = client
        .append_entry(tonic::Request::new(AppendEntryRequest { entry: Some(entry) }))
        .await
        .map_err(|e| anyhow::anyhow!("emergency end failed: {e}"))?;

    let seq = resp.into_inner().sequence;
    Ok(format!("Emergency mode ENDED (seq:{seq}) on vCluster: {vcluster}"))
}

/// Execute `pact approve list` — list pending approvals from PolicyService.
pub async fn approve_list(
    auth_channel: &AuthenticatedChannel,
    scope: Option<&str>,
) -> anyhow::Result<String> {
    use pact_common::proto::policy::ListApprovalsRequest;

    let mut client = auth_channel.policy_client();
    let resp = client
        .list_pending_approvals(tonic::Request::new(ListApprovalsRequest {
            scope_filter: scope.map(str::to_string),
        }))
        .await
        .map_err(|e| anyhow::anyhow!("list approvals failed: {e}"))?;

    let approvals = resp.into_inner().approvals;
    if approvals.is_empty() {
        return Ok("No pending approvals.".to_string());
    }

    let mut output =
        format!("{:<12} {:<20} {:<10} {:<24} {}\n", "ID", "SCOPE", "ACTION", "REQUESTER", "STATUS");
    for a in &approvals {
        let id = if a.approval_id.len() > 10 { &a.approval_id[..10] } else { &a.approval_id };
        output.push_str(&format!(
            "{:<12} {:<20} {:<10} {:<24} {}\n",
            id, a.scope, a.action, a.requester, a.status,
        ));
    }
    Ok(output)
}

/// Execute `pact approve accept/deny` — decide on a pending approval.
pub async fn approve_decide(
    auth_channel: &AuthenticatedChannel,
    approval_id: &str,
    decision: &str,
    principal: &str,
    role: &str,
    reason: Option<&str>,
) -> anyhow::Result<String> {
    use pact_common::proto::policy::DecideApprovalRequest;

    let mut client = auth_channel.policy_client();
    let resp = client
        .decide_approval(tonic::Request::new(DecideApprovalRequest {
            approval_id: approval_id.to_string(),
            approver: Some(ProtoIdentity {
                principal: principal.to_string(),
                principal_type: "Human".to_string(),
                role: role.to_string(),
            }),
            decision: decision.to_string(),
            reason: reason.map(str::to_string),
        }))
        .await
        .map_err(|e| anyhow::anyhow!("decide approval failed: {e}"))?;

    let result = resp.into_inner();
    if result.success {
        Ok(format!("Approval {approval_id}: {decision}"))
    } else {
        Err(anyhow::anyhow!(
            "approval decision failed: {}",
            result.error.unwrap_or_else(|| "unknown error".to_string())
        ))
    }
}

/// Execute `pact watch` — live event stream from journal.
pub async fn watch(auth_channel: &AuthenticatedChannel, vcluster: &str) -> anyhow::Result<String> {
    use pact_common::proto::stream::{config_update, SubscribeRequest};

    let mut client = auth_channel.boot_config_client();
    let resp = client
        .subscribe_config_updates(tonic::Request::new(SubscribeRequest {
            node_id: String::new(), // watch all nodes
            vcluster_id: vcluster.to_string(),
            from_sequence: 0,
        }))
        .await
        .map_err(|e| anyhow::anyhow!("watch subscribe failed: {e}"))?;

    let mut stream = resp.into_inner();
    println!("Watching config updates for vCluster: {vcluster} (Ctrl-C to stop)\n");

    while let Some(result) = tokio_stream::StreamExt::next(&mut stream).await {
        match result {
            Ok(update) => {
                let ts = update.timestamp.as_ref().map_or_else(
                    || "---".to_string(),
                    |t| {
                        chrono::DateTime::from_timestamp(t.seconds, 0).map_or_else(
                            || "---".to_string(),
                            |dt| dt.format("%H:%M:%S").to_string(),
                        )
                    },
                );
                let kind = match &update.update {
                    Some(config_update::Update::VclusterChange(_)) => "OVERLAY",
                    Some(config_update::Update::NodeChange(_)) => "NODE_DELTA",
                    Some(config_update::Update::PolicyChange(_)) => "POLICY",
                    Some(config_update::Update::BlacklistChange(_)) => "BLACKLIST",
                    None => "UNKNOWN",
                };
                println!("[{ts}] seq:{:<6} {kind}", update.sequence);
            }
            Err(e) => {
                eprintln!("Stream error: {e}");
                break;
            }
        }
    }

    Ok("Watch ended.".to_string())
}

/// Execute `pact extend` — extend commit window on agent.
pub async fn extend(channel: Channel, token: &str, mins: u32) -> anyhow::Result<String> {
    use pact_common::proto::shell::ExtendWindowRequest;

    let mut client = AuthenticatedChannel::shell_client_for(channel, token);
    let resp = client
        .extend_commit_window(tonic::Request::new(ExtendWindowRequest { additional_minutes: mins }))
        .await
        .map_err(|e| anyhow::anyhow!("extend failed: {e}"))?;

    let result = resp.into_inner();
    if result.success {
        let secs = result.new_deadline_seconds;
        let mins_remaining = secs / 60;
        Ok(format!("Commit window extended by {mins} minutes ({mins_remaining} minutes remaining)"))
    } else {
        Err(anyhow::anyhow!(
            "extend failed: {}",
            result.error.unwrap_or_else(|| "unknown error".to_string())
        ))
    }
}

/// Execute `pact shell` — open interactive shell session on a node.
pub async fn shell_interactive(channel: Channel, token: &str) -> anyhow::Result<String> {
    use pact_common::proto::shell::{shell_input, shell_output, ShellInput, ShellOpen};
    use tokio_stream::StreamExt;

    let mut client = AuthenticatedChannel::shell_client_for(channel, token);
    let (tx, rx) = tokio::sync::mpsc::channel(64);

    // Send ShellOpen as first message
    let open_msg = ShellInput {
        input: Some(shell_input::Input::Open(ShellOpen {
            rows: 24,
            cols: 80,
            term: std::env::var("TERM").unwrap_or_else(|_| "xterm".into()),
        })),
    };
    tx.send(open_msg).await.ok();

    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
    let request = tonic::Request::new(stream);

    let response =
        client.shell(request).await.map_err(|e| anyhow::anyhow!("shell connection failed: {e}"))?;

    let mut output_stream = response.into_inner();
    while let Some(Ok(msg)) = output_stream.next().await {
        match msg.output {
            Some(shell_output::Output::Stdout(data)) => {
                use std::io::Write;
                std::io::stdout().write_all(&data).ok();
                std::io::stdout().flush().ok();
            }
            Some(shell_output::Output::SessionId(id)) => {
                eprintln!("Shell session: {id}");
            }
            Some(shell_output::Output::ExitCode(code)) => {
                return Ok(format!("Shell exited with code {code}"));
            }
            Some(shell_output::Output::Error(e)) => {
                return Err(anyhow::anyhow!("shell error: {e}"));
            }
            None => {}
        }
    }

    Ok("Shell session ended.".to_string())
}

/// Execute `pact blacklist add` — append a blacklist add entry through Raft.
pub async fn blacklist_add(
    client: &mut AuthConfigClient,
    pattern: &str,
    vcluster: &str,
    principal: &str,
    role: &str,
) -> anyhow::Result<String> {
    let entry = ProtoConfigEntry {
        sequence: 0,
        timestamp: None,
        entry_type: 1, // Commit — blacklist changes are config commits
        scope: Some(ProtoScopeMsg { scope: Some(ProtoScope::VclusterId(vcluster.to_string())) }),
        author: Some(ProtoIdentity {
            principal: principal.to_string(),
            principal_type: "Human".to_string(),
            role: role.to_string(),
        }),
        parent: None,
        state_delta: None,
        policy_ref: format!("blacklist:add:{pattern}"),
        ttl: None,
        emergency_reason: None,
    };

    let resp = client
        .append_entry(tonic::Request::new(AppendEntryRequest { entry: Some(entry) }))
        .await
        .map_err(|e| anyhow::anyhow!("blacklist add failed: {e}"))?;

    let seq = resp.into_inner().sequence;
    let result = super::blacklist::BlacklistResult {
        operation: super::blacklist::BlacklistOp::Add(pattern.to_string()),
        paths: vec![pattern.to_string()],
    };
    Ok(format!(
        "{} (seq:{seq}) on vCluster: {vcluster}",
        super::blacklist::format_blacklist_result(&result)
    ))
}

/// Execute `pact blacklist remove` — append a blacklist remove entry through Raft.
pub async fn blacklist_remove(
    client: &mut AuthConfigClient,
    pattern: &str,
    vcluster: &str,
    principal: &str,
    role: &str,
) -> anyhow::Result<String> {
    let entry = ProtoConfigEntry {
        sequence: 0,
        timestamp: None,
        entry_type: 1, // Commit — blacklist changes are config commits
        scope: Some(ProtoScopeMsg { scope: Some(ProtoScope::VclusterId(vcluster.to_string())) }),
        author: Some(ProtoIdentity {
            principal: principal.to_string(),
            principal_type: "Human".to_string(),
            role: role.to_string(),
        }),
        parent: None,
        state_delta: None,
        policy_ref: format!("blacklist:remove:{pattern}"),
        ttl: None,
        emergency_reason: None,
    };

    let resp = client
        .append_entry(tonic::Request::new(AppendEntryRequest { entry: Some(entry) }))
        .await
        .map_err(|e| anyhow::anyhow!("blacklist remove failed: {e}"))?;

    let seq = resp.into_inner().sequence;
    let result = super::blacklist::BlacklistResult {
        operation: super::blacklist::BlacklistOp::Remove(pattern.to_string()),
        paths: vec![pattern.to_string()],
    };
    Ok(format!(
        "{} (seq:{seq}) on vCluster: {vcluster}",
        super::blacklist::format_blacklist_result(&result)
    ))
}

/// Execute `pact group list` — discover vClusters via journal entries and query their policies.
pub async fn group_list(auth_channel: &AuthenticatedChannel) -> anyhow::Result<String> {
    use super::group::{format_group_list, GroupSummary};
    use pact_common::proto::policy::GetPolicyRequest;
    use std::collections::BTreeSet;

    // List all journal entries (no scope filter) and collect unique vCluster IDs.
    let mut config_client = auth_channel.config_client();
    let resp = config_client
        .list_entries(tonic::Request::new(ListEntriesRequest {
            scope: None,
            from_sequence: None,
            to_sequence: None,
            limit: None,
        }))
        .await
        .map_err(|e| anyhow::anyhow!("list entries failed: {e}"))?;

    let mut stream = resp.into_inner();
    let mut vcluster_ids = BTreeSet::new();

    while let Some(entry) = tokio_stream::StreamExt::next(&mut stream).await {
        match entry {
            Ok(e) => {
                if let Some(ref scope) = e.scope {
                    if let Some(ProtoScope::VclusterId(ref vc)) = scope.scope {
                        vcluster_ids.insert(vc.clone());
                    }
                }
            }
            Err(e) => return Err(anyhow::anyhow!("list entries stream error: {e}")),
        }
    }

    if vcluster_ids.is_empty() {
        return Ok("No vClusters configured.".to_string());
    }

    // Query effective policy for each discovered vCluster
    let mut policy_client = auth_channel.policy_client();
    let mut summaries = Vec::new();

    for vc_id in &vcluster_ids {
        match policy_client
            .get_effective_policy(tonic::Request::new(GetPolicyRequest {
                vcluster_id: vc_id.clone(),
            }))
            .await
        {
            Ok(resp) => {
                let policy = resp.into_inner();
                summaries.push(GroupSummary {
                    name: vc_id.clone(),
                    node_count: 0, // node count requires enrollment service
                    enforcement_mode: if policy.enforcement_mode.is_empty() {
                        "observe".to_string()
                    } else {
                        policy.enforcement_mode
                    },
                    two_person_approval: policy.two_person_approval,
                });
            }
            Err(_) => {
                // vCluster exists in journal but has no policy yet — show with defaults
                summaries.push(GroupSummary {
                    name: vc_id.clone(),
                    node_count: 0,
                    enforcement_mode: "observe".to_string(),
                    two_person_approval: false,
                });
            }
        }
    }

    Ok(format_group_list(&summaries))
}

/// Execute `pact group show` — show details for a specific vCluster.
pub async fn group_show(auth_channel: &AuthenticatedChannel, name: &str) -> anyhow::Result<String> {
    use super::group::{format_group_detail, GroupDetail};
    use pact_common::proto::policy::GetPolicyRequest;
    use pact_common::types::VClusterPolicy;

    let mut client = auth_channel.policy_client();
    let resp = client
        .get_effective_policy(tonic::Request::new(GetPolicyRequest {
            vcluster_id: name.to_string(),
        }))
        .await
        .map_err(|e| anyhow::anyhow!("get policy for '{name}' failed: {e}"))?;

    let proto_policy = resp.into_inner();

    let policy = VClusterPolicy {
        vcluster_id: proto_policy.vcluster_id.clone(),
        policy_id: proto_policy.policy_id.clone(),
        updated_at: proto_policy
            .updated_at
            .as_ref()
            .and_then(|ts| chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32)),
        drift_sensitivity: proto_policy.drift_sensitivity,
        base_commit_window_seconds: proto_policy.base_commit_window_seconds,
        emergency_window_seconds: proto_policy.emergency_window_seconds,
        auto_converge_categories: proto_policy.auto_converge_categories.clone(),
        require_ack_categories: proto_policy.require_ack_categories.clone(),
        enforcement_mode: if proto_policy.enforcement_mode.is_empty() {
            "observe".to_string()
        } else {
            proto_policy.enforcement_mode.clone()
        },
        role_bindings: proto_policy
            .role_bindings
            .iter()
            .map(|rb| pact_common::types::RoleBinding {
                role: rb.role.clone(),
                principals: rb.principals.clone(),
                allowed_actions: rb.allowed_actions.clone(),
            })
            .collect(),
        regulated: proto_policy.regulated,
        two_person_approval: proto_policy.two_person_approval,
        emergency_allowed: proto_policy.emergency_allowed,
        audit_retention_days: proto_policy.audit_retention_days,
        federation_template: proto_policy.federation_template.clone(),
        supervisor_backend: if proto_policy.supervisor_backend.is_empty() {
            "pact".to_string()
        } else {
            proto_policy.supervisor_backend.clone()
        },
        ai_exec_allowed: proto_policy.ai_exec_allowed,
        exec_whitelist: proto_policy.exec_whitelist.clone(),
        shell_whitelist: proto_policy.shell_whitelist.clone(),
    };

    let detail = GroupDetail { name: name.to_string(), policy, node_ids: vec![] };

    Ok(format_group_detail(&detail))
}

/// Execute `pact group set-policy` — update a vCluster's policy from a TOML file.
pub async fn group_set_policy(
    auth_channel: &AuthenticatedChannel,
    name: &str,
    policy_path: &str,
    principal: &str,
    role: &str,
) -> anyhow::Result<String> {
    use pact_common::proto::policy::{
        RoleBinding as ProtoRoleBinding, UpdatePolicyRequest, VClusterPolicy as ProtoVClusterPolicy,
    };
    use pact_common::types::VClusterPolicy;

    let toml_content = std::fs::read_to_string(policy_path)
        .map_err(|e| anyhow::anyhow!("cannot read policy file {policy_path}: {e}"))?;

    let policy: VClusterPolicy = toml::from_str(&toml_content)
        .map_err(|e| anyhow::anyhow!("invalid policy TOML in {policy_path}: {e}"))?;

    let proto_policy = ProtoVClusterPolicy {
        vcluster_id: name.to_string(),
        policy_id: policy.policy_id,
        updated_at: None, // server assigns timestamp
        drift_sensitivity: policy.drift_sensitivity,
        base_commit_window_seconds: policy.base_commit_window_seconds,
        emergency_window_seconds: policy.emergency_window_seconds,
        auto_converge_categories: policy.auto_converge_categories,
        require_ack_categories: policy.require_ack_categories,
        enforcement_mode: policy.enforcement_mode,
        role_bindings: policy
            .role_bindings
            .into_iter()
            .map(|rb| ProtoRoleBinding {
                role: rb.role,
                principals: rb.principals,
                allowed_actions: rb.allowed_actions,
            })
            .collect(),
        regulated: policy.regulated,
        two_person_approval: policy.two_person_approval,
        audit_retention_days: policy.audit_retention_days,
        federation_template: policy.federation_template,
        supervisor_backend: policy.supervisor_backend,
        exec_whitelist: policy.exec_whitelist,
        shell_whitelist: policy.shell_whitelist,
        emergency_allowed: policy.emergency_allowed,
        ai_exec_allowed: policy.ai_exec_allowed,
    };

    let mut client = auth_channel.policy_client();
    let resp = client
        .update_policy(tonic::Request::new(UpdatePolicyRequest {
            vcluster_id: name.to_string(),
            policy: Some(proto_policy),
            author: Some(ProtoIdentity {
                principal: principal.to_string(),
                principal_type: "Human".to_string(),
                role: role.to_string(),
            }),
            message: format!("Policy update from {policy_path}"),
        }))
        .await
        .map_err(|e| anyhow::anyhow!("update policy failed: {e}"))?;

    let result = resp.into_inner();
    if result.success {
        Ok(format!("Policy updated for vCluster '{name}' (ref: {})", result.policy_ref))
    } else {
        Err(anyhow::anyhow!(
            "policy update failed: {}",
            result.error.unwrap_or_else(|| "unknown error".to_string())
        ))
    }
}

/// Parse a scope filter string (e.g. "node:X", "vc:X", "global") to proto Scope.
fn parse_scope_filter(s: &str) -> ProtoScopeMsg {
    if let Some(node) = s.strip_prefix("node:") {
        ProtoScopeMsg { scope: Some(ProtoScope::NodeId(node.to_string())) }
    } else if let Some(vc) = s.strip_prefix("vc:") {
        ProtoScopeMsg { scope: Some(ProtoScope::VclusterId(vc.to_string())) }
    } else {
        ProtoScopeMsg { scope: Some(ProtoScope::Global(true)) }
    }
}

/// Format a proto ConfigEntry for display.
fn format_proto_entry(entry: &ProtoConfigEntry) -> String {
    let entry_type_name = match entry.entry_type {
        1 => "COMMIT",
        2 => "ROLLBACK",
        3 => "AUTO_CONVERGE",
        4 => "DRIFT_DETECTED",
        5 => "CAPABILITY_CHANGE",
        6 => "POLICY_UPDATE",
        7 => "BOOT_CONFIG",
        8 => "EMERGENCY_ON",
        9 => "EMERGENCY_OFF",
        10 => "EXEC_LOG",
        11 => "SHELL_SESSION",
        12 => "SERVICE_LIFECYCLE",
        13 => "PENDING_APPROVAL",
        _ => "UNKNOWN",
    };

    let scope = entry.scope.as_ref().map_or_else(
        || "global".to_string(),
        |s| match &s.scope {
            Some(ProtoScope::NodeId(n)) => format!("node:{n}"),
            Some(ProtoScope::VclusterId(v)) => format!("vc:{v}"),
            _ => "global".to_string(),
        },
    );

    let author =
        entry.author.as_ref().map_or_else(|| "unknown".to_string(), |a| a.principal.clone());

    let timestamp = entry.timestamp.as_ref().map_or_else(
        || "---".to_string(),
        |ts| {
            chrono::DateTime::from_timestamp(ts.seconds, 0)
                .map_or_else(|| "---".to_string(), |dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        },
    );

    format!(
        "#{:<6} {} {:<18} {:<20} by {}",
        entry.sequence, timestamp, entry_type_name, scope, author
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_scope_filter_node() {
        let scope = parse_scope_filter("node:node-042");
        assert!(matches!(scope.scope, Some(ProtoScope::NodeId(ref n)) if n == "node-042"));
    }

    #[test]
    fn parse_scope_filter_vcluster() {
        let scope = parse_scope_filter("vc:ml-training");
        assert!(matches!(scope.scope, Some(ProtoScope::VclusterId(ref v)) if v == "ml-training"));
    }

    #[test]
    fn parse_scope_filter_global() {
        let scope = parse_scope_filter("global");
        assert!(matches!(scope.scope, Some(ProtoScope::Global(true))));
    }

    #[test]
    fn format_proto_entry_commit() {
        let entry = ProtoConfigEntry {
            sequence: 42,
            timestamp: Some(prost_types::Timestamp { seconds: 1_741_622_400, nanos: 0 }),
            entry_type: 1,
            scope: Some(ProtoScopeMsg {
                scope: Some(ProtoScope::VclusterId("ml-training".into())),
            }),
            author: Some(ProtoIdentity {
                principal: "admin@example.com".into(),
                principal_type: "Human".into(),
                role: "pact-platform-admin".into(),
            }),
            parent: None,
            state_delta: None,
            policy_ref: String::new(),
            ttl: None,
            emergency_reason: None,
        };
        let formatted = format_proto_entry(&entry);
        assert!(formatted.contains("#42"));
        assert!(formatted.contains("COMMIT"));
        assert!(formatted.contains("vc:ml-training"));
        assert!(formatted.contains("admin@example.com"));
    }

    #[test]
    fn format_proto_entry_emergency() {
        let entry = ProtoConfigEntry {
            sequence: 100,
            timestamp: None,
            entry_type: 8, // EmergencyStart
            scope: Some(ProtoScopeMsg { scope: Some(ProtoScope::Global(true)) }),
            author: Some(ProtoIdentity {
                principal: "ops@example.com".into(),
                principal_type: "Human".into(),
                role: "pact-ops-ml".into(),
            }),
            parent: None,
            state_delta: None,
            policy_ref: String::new(),
            ttl: None,
            emergency_reason: Some("GPU failure".into()),
        };
        let formatted = format_proto_entry(&entry);
        assert!(formatted.contains("#100"));
        assert!(formatted.contains("EMERGENCY_ON"));
        assert!(formatted.contains("global"));
    }

    #[test]
    fn resolve_identity_from_valid_jwt() {
        // Create a real JWT with jsonwebtoken
        use jsonwebtoken::{encode, EncodingKey, Header};

        #[derive(serde::Serialize)]
        struct Claims {
            sub: String,
            pact_role: String,
            exp: u64,
        }
        let token = encode(
            &Header::default(),
            &Claims {
                sub: "alice@example.com".into(),
                pact_role: "pact-ops-ml-training".into(),
                exp: 9_999_999_999,
            },
            &EncodingKey::from_secret(b"test-secret"),
        )
        .unwrap();

        let (principal, role) = resolve_identity_from_token(&token);
        assert_eq!(principal, "alice@example.com");
        assert_eq!(role, "pact-ops-ml-training");
    }

    #[test]
    fn resolve_identity_from_invalid_token_returns_defaults() {
        let (principal, role) = resolve_identity_from_token("not-a-jwt");
        assert_eq!(principal, "cli-user");
        assert_eq!(role, "pact-platform-admin");
    }

    #[test]
    fn resolve_identity_from_empty_token_returns_defaults() {
        let (principal, role) = resolve_identity_from_token("");
        assert_eq!(principal, "cli-user");
        assert_eq!(role, "pact-platform-admin");
    }

    #[test]
    fn tls_options_default_is_empty() {
        let opts = TlsOptions::default();
        assert!(opts.ca_cert.is_none());
        assert!(opts.client_cert.is_none());
        assert!(opts.client_key.is_none());
    }

    #[test]
    fn tls_options_nonexistent_ca_cert_returns_error() {
        let opts = TlsOptions {
            ca_cert: Some("/nonexistent/ca.pem".into()),
            client_cert: None,
            client_key: None,
        };
        let result = opts.to_tls_config();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot read CA cert"));
    }

    #[test]
    fn tls_options_nonexistent_client_cert_returns_error() {
        let opts = TlsOptions {
            ca_cert: None,
            client_cert: Some("/nonexistent/cert.pem".into()),
            client_key: Some("/nonexistent/key.pem".into()),
        };
        let result = opts.to_tls_config();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("cannot read client cert"));
    }

    #[test]
    fn resolve_agent_address_local_returns_localhost() {
        // resolve_agent_address is async but for "local" it doesn't use the channel
        let rt = tokio::runtime::Runtime::new().unwrap();
        // We need a dummy AuthenticatedChannel — but for "local" it short-circuits
        rt.block_on(async {
            let channel = Channel::from_static("http://127.0.0.1:1").connect_lazy();
            let auth = AuthenticatedChannel::new(channel, String::new());
            let addr = resolve_agent_address("local", &auth).await.unwrap();
            assert_eq!(addr, "http://127.0.0.1:9445");
        });
    }

    #[test]
    fn resolve_agent_address_localhost_returns_localhost() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let channel = Channel::from_static("http://127.0.0.1:1").connect_lazy();
            let auth = AuthenticatedChannel::new(channel, String::new());
            let addr = resolve_agent_address("localhost", &auth).await.unwrap();
            assert_eq!(addr, "http://127.0.0.1:9445");
        });
    }
}
