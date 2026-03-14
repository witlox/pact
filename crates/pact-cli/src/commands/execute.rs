//! Command execution — gRPC client calls to journal services.
//!
//! Each function connects to the journal, executes the request,
//! and returns the formatted result.

use tonic::transport::Channel;
use tracing::debug;

use pact_common::proto::config::{
    scope::Scope as ProtoScope, ConfigEntry as ProtoConfigEntry, Identity as ProtoIdentity,
    Scope as ProtoScopeMsg,
};
use pact_common::proto::journal::config_service_client::ConfigServiceClient;
use pact_common::proto::journal::{
    AppendEntryRequest, GetNodeStateRequest, ListEntriesRequest,
};

use super::config::CliConfig;

/// Create a gRPC channel to the journal endpoint.
pub async fn connect(config: &CliConfig) -> anyhow::Result<Channel> {
    let uri = if config.endpoint.starts_with("http") {
        config.endpoint.clone()
    } else {
        format!("http://{}", config.endpoint)
    };

    let channel = Channel::from_shared(uri.clone())
        .map_err(|e| anyhow::anyhow!("invalid endpoint {uri}: {e}"))?
        .timeout(std::time::Duration::from_secs(u64::from(config.timeout_seconds)))
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
        scope: Some(ProtoScopeMsg {
            scope: Some(ProtoScope::VclusterId(vcluster.to_string())),
        }),
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
        scope: Some(ProtoScopeMsg {
            scope: Some(ProtoScope::VclusterId(vcluster.to_string())),
        }),
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

    let author = entry
        .author
        .as_ref()
        .map_or_else(|| "unknown".to_string(), |a| a.principal.clone());

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
            timestamp: Some(prost_types::Timestamp { seconds: 1741622400, nanos: 0 }),
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
}
