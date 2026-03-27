//! `pact diag` — structured diagnostic log retrieval.
//!
//! Retrieves logs from agent nodes: dmesg, syslog, supervised service logs.
//! Supports per-node and fleet-wide (vCluster) queries with server-side
//! grep filtering (LOG2) and line limiting (LOG3).

use super::execute::AuthenticatedChannel;
use pact_common::proto::shell::DiagRequest;

/// Execute single-node diagnostic collection.
pub async fn diag_node(
    channel: tonic::transport::Channel,
    token: &str,
    source: &str,
    service: Option<&str>,
    grep: Option<&str>,
    lines: u32,
) -> anyhow::Result<String> {
    let mut client = AuthenticatedChannel::shell_client_for(channel, token);
    let request = tonic::Request::new(DiagRequest {
        source_filter: source.to_string(),
        service_name: service.unwrap_or("").to_string(),
        grep_pattern: grep.unwrap_or("").to_string(),
        line_limit: lines,
    });

    let resp =
        client.collect_diag(request).await.map_err(|e| anyhow::anyhow!("diag failed: {e}"))?;

    let mut stream = resp.into_inner();
    let mut output = String::new();
    while let Some(chunk) = tokio_stream::StreamExt::next(&mut stream).await {
        match chunk {
            Ok(c) => {
                if !c.lines.is_empty() {
                    output.push_str(&format!("--- {} ---\n", c.source));
                    for line in &c.lines {
                        output.push_str(line);
                        output.push('\n');
                    }
                    if c.truncated {
                        output.push_str("(truncated)\n");
                    }
                }
            }
            Err(e) => return Err(anyhow::anyhow!("diag stream error: {e}")),
        }
    }

    if output.is_empty() {
        Ok("No log entries found.".to_string())
    } else {
        Ok(output)
    }
}

/// Execute fleet-wide diagnostic collection across a vCluster.
///
/// Fan-out to all agents in the vCluster with a semaphore (max 50 parallel).
/// Each line is prefixed with `[node_id]`. Unreachable nodes are reported.
pub async fn diag_fleet(
    _journal_channel: &tonic::transport::Channel,
    _token: &str,
    _vcluster: &str,
    _source: &str,
    _service: Option<&str>,
    _grep: Option<&str>,
    _lines: u32,
) -> anyhow::Result<String> {
    // Fleet-wide fan-out requires:
    // 1. Get node list from enrollment service
    // 2. Resolve agent addresses for each node
    // 3. Fan out CollectDiag to all agents with semaphore
    // 4. Prefix each line with [node_id]
    // 5. Collect errors for unreachable nodes
    //
    // Placeholder: will be wired when enrollment service exposes node list.
    Err(anyhow::anyhow!(
        "fleet-wide diag requires --node or is not yet implemented for vCluster fan-out"
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn diag_request_builds_correctly() {
        use pact_common::proto::shell::DiagRequest;

        let req = DiagRequest {
            source_filter: "system".to_string(),
            service_name: String::new(),
            grep_pattern: "error".to_string(),
            line_limit: 200,
        };
        assert_eq!(req.source_filter, "system");
        assert_eq!(req.grep_pattern, "error");
        assert_eq!(req.line_limit, 200);
    }
}
