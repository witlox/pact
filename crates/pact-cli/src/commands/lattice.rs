//! Lattice supercharged commands — read operations delegated to lattice scheduler.
//!
//! These commands query the lattice scheduler for job/allocation status, queue info,
//! audit logs, accounting, and cluster health. Combined commands (cluster status,
//! health, audit) query both pact journal and lattice.

use pact_common::config::DelegationConfig;
use pact_common::proto::journal::config_service_client::ConfigServiceClient;
use tonic::transport::Channel;

/// Connect to the lattice scheduler, returning a client or an error.
async fn connect_lattice(
    config: &DelegationConfig,
) -> anyhow::Result<lattice_client::LatticeClient> {
    let Some(ref endpoint) = config.lattice_endpoint else {
        anyhow::bail!("lattice endpoint not configured (set PACT_LATTICE_ENDPOINT)");
    };

    let client_config = lattice_client::ClientConfig {
        endpoint: endpoint.clone(),
        timeout_secs: config.timeout_secs,
        token: config.lattice_token.clone(),
    };

    lattice_client::LatticeClient::connect(client_config)
        .await
        .map_err(|e| anyhow::anyhow!("lattice connection failed: {e}"))
}

// ─── Jobs ──────────────────────────────────────────────────

/// List allocations (jobs) from lattice, with optional node/vCluster filters.
pub async fn list_jobs(
    config: &DelegationConfig,
    node_filter: Option<&str>,
    vcluster_filter: Option<&str>,
) -> anyhow::Result<String> {
    let mut lc = connect_lattice(config).await?;

    let req = lattice_client::proto::ListAllocationsRequest {
        vcluster: vcluster_filter.unwrap_or_default().to_string(),
        ..Default::default()
    };

    let resp = lc.list_allocations(req).await.map_err(|e| anyhow::anyhow!("{e}"))?;

    if resp.allocations.is_empty() {
        return Ok("No jobs found.".to_string());
    }

    let mut out = String::new();
    out.push_str(&format!(
        "{:<36}  {:<12}  {:<20}  {:<16}  {}\n",
        "ID", "STATE", "NODE(S)", "VCLUSTER", "USER"
    ));
    out.push_str(&"-".repeat(100));
    out.push('\n');

    for alloc in &resp.allocations {
        // Apply node filter client-side if specified
        if let Some(node) = node_filter {
            if !alloc.assigned_nodes.iter().any(|n| n == node) {
                continue;
            }
        }

        let nodes = if alloc.assigned_nodes.is_empty() {
            "-".to_string()
        } else if alloc.assigned_nodes.len() <= 2 {
            alloc.assigned_nodes.join(",")
        } else {
            format!("{}+{}", alloc.assigned_nodes[0], alloc.assigned_nodes.len() - 1)
        };

        let vc = alloc.spec.as_ref().map_or("-", |s| s.vcluster.as_str());

        out.push_str(&format!(
            "{:<36}  {:<12}  {:<20}  {:<16}  {}\n",
            alloc.allocation_id, alloc.state, nodes, vc, alloc.user,
        ));
    }

    Ok(out)
}

/// Cancel a job/allocation.
pub async fn cancel_job(config: &DelegationConfig, job_id: &str) -> anyhow::Result<String> {
    let mut lc = connect_lattice(config).await?;

    let resp = lc.cancel(job_id).await.map_err(|e| anyhow::anyhow!("{e}"))?;

    if resp.success {
        Ok(format!("Job {job_id} cancelled."))
    } else {
        Ok(format!("Job {job_id} cancel returned: success=false"))
    }
}

/// Inspect a single job/allocation — show all fields.
pub async fn inspect_job(config: &DelegationConfig, job_id: &str) -> anyhow::Result<String> {
    let mut lc = connect_lattice(config).await?;

    let status = lc.get_allocation(job_id).await.map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut out = String::new();
    out.push_str(&format!("Allocation: {}\n", status.allocation_id));
    out.push_str(&format!("State:      {}\n", status.state));
    out.push_str(&format!("User:       {}\n", status.user));
    out.push_str(&format!("Message:    {}\n", status.message));
    out.push_str(&format!("Exit code:  {}\n", status.exit_code));

    if !status.assigned_nodes.is_empty() {
        out.push_str(&format!("Nodes:      {}\n", status.assigned_nodes.join(", ")));
    }

    if let Some(spec) = &status.spec {
        out.push_str(&format!("Tenant:     {}\n", spec.tenant));
        out.push_str(&format!("Project:    {}\n", spec.project));
        out.push_str(&format!("vCluster:   {}\n", spec.vcluster));
        out.push_str(&format!("Entrypoint: {}\n", spec.entrypoint));
        out.push_str(&format!("Sensitive:  {}\n", spec.sensitive));
        if let Some(ref res) = spec.resources {
            out.push_str(&format!(
                "Resources:  min_nodes={}, max_nodes={}, gpu_type={}\n",
                res.min_nodes, res.max_nodes, res.gpu_type
            ));
        }
    }

    if let Some(ref ts) = status.created_at {
        out.push_str(&format!("Created:    {}s\n", ts.seconds));
    }
    if let Some(ref ts) = status.started_at {
        out.push_str(&format!("Started:    {}s\n", ts.seconds));
    }
    if let Some(ref ts) = status.completed_at {
        out.push_str(&format!("Completed:  {}s\n", ts.seconds));
    }

    Ok(out)
}

// ─── Queue ─────────────────────────────────────────────────

/// Show scheduling queue status for a vCluster.
pub async fn queue_status(
    config: &DelegationConfig,
    vcluster: Option<&str>,
) -> anyhow::Result<String> {
    let mut lc = connect_lattice(config).await?;

    let vc_id = vcluster.unwrap_or("default");
    let resp = lc.vcluster_queue(vc_id).await.map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut out = String::new();
    out.push_str(&format!("Queue status for vCluster: {}\n", resp.vcluster_id));
    out.push_str(&format!("  Pending: {}\n", resp.pending));
    out.push_str(&format!("  Running: {}\n", resp.running));
    out.push_str(&format!("  Total:   {}\n", resp.total));

    Ok(out)
}

// ─── Cluster Status ────────────────────────────────────────

/// Combined cluster status — query both pact journal Raft and lattice Raft.
pub async fn cluster_status(
    journal_client: &mut ConfigServiceClient<Channel>,
    config: &DelegationConfig,
) -> anyhow::Result<String> {
    let mut out = String::new();

    // --- Pact journal status ---
    out.push_str("=== Pact Journal ===\n");
    match super::execute::status(journal_client, "local").await {
        Ok(status) => out.push_str(&format!("{status}\n")),
        Err(e) => out.push_str(&format!("  Status: error ({e})\n")),
    }

    // --- Lattice Raft status ---
    out.push_str("\n=== Lattice Scheduler ===\n");
    match connect_lattice(config).await {
        Err(msg) => out.push_str(&format!("  {msg}\n")),
        Ok(mut lc) => match lc.raft_status().await {
            Ok(raft) => {
                out.push_str(&format!("  Leader:       {}\n", raft.leader_id));
                out.push_str(&format!("  Term:         {}\n", raft.current_term));
                out.push_str(&format!("  Last applied: {}\n", raft.last_applied));
                out.push_str(&format!("  Commit index: {}\n", raft.commit_index));
                if !raft.members.is_empty() {
                    out.push_str("  Members:\n");
                    for m in &raft.members {
                        out.push_str(&format!(
                            "    {} ({}) — role={}, match_index={}\n",
                            m.node_id, m.address, m.role, m.match_index
                        ));
                    }
                }
            }
            Err(e) => out.push_str(&format!("  Raft query failed: {e}\n")),
        },
    }

    Ok(out)
}

// ─── Audit ─────────────────────────────────────────────────

/// Combined audit log — query pact journal and/or lattice audit.
pub async fn audit_combined(
    journal_client: &mut ConfigServiceClient<Channel>,
    config: &DelegationConfig,
    source: &str,
    limit: u32,
) -> anyhow::Result<String> {
    let mut out = String::new();
    let show_pact = source == "all" || source == "pact";
    let show_lattice = source == "all" || source == "lattice";

    if show_pact {
        out.push_str("=== Pact Audit Log ===\n");
        match super::execute::log(journal_client, limit, None).await {
            Ok(log) => out.push_str(&format!("{log}\n")),
            Err(e) => out.push_str(&format!("  Error: {e}\n")),
        }
    }

    if show_lattice {
        out.push_str("=== Lattice Audit Log ===\n");
        match connect_lattice(config).await {
            Err(msg) => out.push_str(&format!("  {msg}\n")),
            Ok(mut lc) => {
                let req = lattice_client::proto::QueryAuditRequest { limit, ..Default::default() };
                match lc.query_audit(req).await {
                    Ok(resp) => {
                        if resp.entries.is_empty() {
                            out.push_str("  No audit entries.\n");
                        }
                        for entry in &resp.entries {
                            let ts = entry
                                .timestamp
                                .as_ref()
                                .map_or_else(|| "-".into(), |t| format!("{}s", t.seconds));
                            out.push_str(&format!(
                                "  [{}] {} {} — {}\n",
                                ts, entry.user_id, entry.action, entry.details
                            ));
                        }
                    }
                    Err(e) => out.push_str(&format!("  Audit query failed: {e}\n")),
                }
            }
        }
    }

    Ok(out)
}

// ─── Accounting ────────────────────────────────────────────

/// Resource accounting from lattice.
pub async fn accounting(
    config: &DelegationConfig,
    vcluster: Option<&str>,
) -> anyhow::Result<String> {
    let mut lc = connect_lattice(config).await?;

    // vCluster maps to tenant in lattice accounting
    let tenant_id = vcluster.unwrap_or_default().to_string();
    let req = lattice_client::proto::GetAccountingUsageRequest { tenant_id, ..Default::default() };

    let resp = lc.accounting_usage(req).await.map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut out = String::new();
    out.push_str(&format!(
        "{:<16}  {:<14}  {:<14}  {:<16}  {}\n",
        "TENANT", "CPU HOURS", "GPU HOURS", "STORAGE (bytes)", "ALLOCATIONS"
    ));
    out.push_str(&"-".repeat(80));
    out.push('\n');
    out.push_str(&format!(
        "{:<16}  {:<14.1}  {:<14.1}  {:<16}  {}\n",
        resp.tenant_id, resp.cpu_hours, resp.gpu_hours, resp.storage_bytes, resp.allocation_count,
    ));

    Ok(out)
}

// ─── Health ────────────────────────────────────────────────

/// Combined health check — pact journal + lattice.
pub async fn health_check(
    journal_client: &mut ConfigServiceClient<Channel>,
    config: &DelegationConfig,
) -> anyhow::Result<String> {
    let mut out = String::new();
    let mut all_healthy = true;

    // --- Pact journal health ---
    out.push_str("=== Pact Journal ===\n");
    match super::execute::status(journal_client, "local").await {
        Ok(_) => out.push_str("  Status: HEALTHY\n"),
        Err(e) => {
            out.push_str(&format!("  Status: UNHEALTHY ({e})\n"));
            all_healthy = false;
        }
    }

    // --- Lattice health ---
    out.push_str("\n=== Lattice Scheduler ===\n");
    match connect_lattice(config).await {
        Err(msg) => {
            out.push_str(&format!("  Status: UNAVAILABLE ({msg})\n"));
            all_healthy = false;
        }
        Ok(mut lc) => match lc.health().await {
            Ok(resp) => {
                let healthy = resp.status == "healthy";
                if !healthy {
                    all_healthy = false;
                }
                out.push_str(&format!("  Status:  {}\n", resp.status.to_uppercase()));
                out.push_str(&format!("  Version: {}\n", resp.version));
                out.push_str(&format!("  Uptime:  {}s\n", resp.uptime_secs));
            }
            Err(e) => {
                out.push_str(&format!("  Status: UNHEALTHY ({e})\n"));
                all_healthy = false;
            }
        },
    }

    out.push_str(&format!("\nOverall: {}", if all_healthy { "PASS" } else { "FAIL" }));

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn list_jobs_no_endpoint() {
        let config = DelegationConfig::default();
        let result = list_jobs(&config, None, None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not configured"));
    }

    #[tokio::test]
    async fn cancel_job_no_endpoint() {
        let config = DelegationConfig::default();
        let result = cancel_job(&config, "job-123").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not configured"));
    }

    #[tokio::test]
    async fn inspect_job_no_endpoint() {
        let config = DelegationConfig::default();
        let result = inspect_job(&config, "job-123").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not configured"));
    }

    #[tokio::test]
    async fn queue_status_no_endpoint() {
        let config = DelegationConfig::default();
        let result = queue_status(&config, None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not configured"));
    }

    #[tokio::test]
    async fn accounting_no_endpoint() {
        let config = DelegationConfig::default();
        let result = accounting(&config, None).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not configured"));
    }
}
