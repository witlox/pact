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

/// Resolve identity (principal + role) from a JWT token.
///
/// Decodes the JWT payload without signature verification (the journal validates it).
/// Falls back to defaults if decoding fails.
pub fn resolve_identity_from_token(token: &str) -> (String, String) {
    use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};

    #[derive(serde::Deserialize)]
    struct Claims {
        sub: Option<String>,
        pact_role: Option<String>,
    }

    // Try to decode without verification — we just need the claims
    let mut validation = Validation::new(Algorithm::HS256);
    validation.insecure_disable_signature_validation();
    validation.validate_exp = false;
    validation.validate_aud = false;

    match decode::<Claims>(token, &DecodingKey::from_secret(b""), &validation) {
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

/// Execute `pact status` — query node state from journal.
pub async fn status(
    client: &mut ConfigServiceClient<Channel>,
    node_id: &str,
) -> anyhow::Result<String> {
    let resp = client
        .get_node_state(tonic::Request::new(GetNodeStateRequest { node_id: node_id.to_string() }))
        .await
        .map_err(|e| anyhow::anyhow!("status query failed: {e}"))?;

    let ns = resp.into_inner();
    Ok(format!("Node: {}  State: {}", ns.node_id, ns.config_state))
}

/// Execute `pact log` — list recent config entries from journal.
pub async fn log(
    client: &mut ConfigServiceClient<Channel>,
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
    client: &mut ConfigServiceClient<Channel>,
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
    client: &mut ConfigServiceClient<Channel>,
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
    journal_channel: &Channel,
) -> anyhow::Result<String> {
    if node_id == "local" || node_id == "localhost" {
        return Ok(format!("http://127.0.0.1:{AGENT_DEFAULT_PORT}"));
    }

    // Verify the node exists by querying journal
    let mut client = ConfigServiceClient::new(journal_channel.clone());
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
    use pact_common::proto::shell::{
        exec_output, shell_service_client::ShellServiceClient, ExecRequest,
    };

    let mut client = ShellServiceClient::new(channel);

    let mut request =
        tonic::Request::new(ExecRequest { command: command.to_string(), args: args.to_vec() });
    request.metadata_mut().insert(
        "authorization",
        format!("Bearer {token}").parse().map_err(|_| anyhow::anyhow!("invalid token format"))?,
    );

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
pub async fn list_agent_commands(channel: Channel) -> anyhow::Result<String> {
    use pact_common::proto::shell::{
        shell_service_client::ShellServiceClient, ListCommandsRequest,
    };

    let mut client = ShellServiceClient::new(channel);
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
    client: &mut ConfigServiceClient<Channel>,
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

/// Execute `pact emergency start` — append EmergencyStart entry through Raft.
pub async fn emergency_start(
    client: &mut ConfigServiceClient<Channel>,
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
    client: &mut ConfigServiceClient<Channel>,
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
pub async fn approve_list(channel: &Channel, scope: Option<&str>) -> anyhow::Result<String> {
    use pact_common::proto::policy::{
        policy_service_client::PolicyServiceClient, ListApprovalsRequest,
    };

    let mut client = PolicyServiceClient::new(channel.clone());
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
    channel: &Channel,
    approval_id: &str,
    decision: &str,
    principal: &str,
    role: &str,
    reason: Option<&str>,
) -> anyhow::Result<String> {
    use pact_common::proto::policy::{
        policy_service_client::PolicyServiceClient, DecideApprovalRequest,
    };

    let mut client = PolicyServiceClient::new(channel.clone());
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
pub async fn watch(channel: &Channel, vcluster: &str) -> anyhow::Result<String> {
    use pact_common::proto::stream::{
        boot_config_service_client::BootConfigServiceClient, config_update, SubscribeRequest,
    };

    let mut client = BootConfigServiceClient::new(channel.clone());
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
pub async fn extend(channel: Channel, mins: u32) -> anyhow::Result<String> {
    use pact_common::proto::shell::{
        shell_service_client::ShellServiceClient, ExtendWindowRequest,
    };

    let mut client = ShellServiceClient::new(channel);
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
        // We need a dummy channel — but for "local" it short-circuits
        // Use a simple blocking check
        rt.block_on(async {
            // Create a dummy channel (won't be used for "local")
            let channel = Channel::from_static("http://127.0.0.1:1").connect_lazy();
            let addr = resolve_agent_address("local", &channel).await.unwrap();
            assert_eq!(addr, "http://127.0.0.1:9445");
        });
    }

    #[test]
    fn resolve_agent_address_localhost_returns_localhost() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let channel = Channel::from_static("http://127.0.0.1:1").connect_lazy();
            let addr = resolve_agent_address("localhost", &channel).await.unwrap();
            assert_eq!(addr, "http://127.0.0.1:9445");
        });
    }
}
