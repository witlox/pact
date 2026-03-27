//! Lattice supercharged commands — read operations delegated to lattice scheduler.
//!
//! These commands query the lattice scheduler for job/allocation status, queue info,
//! audit logs, accounting, and cluster health. Combined commands (cluster status,
//! health, audit) query both pact journal and lattice.

use pact_common::config::DelegationConfig;

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
        if let Some(ref probe) = spec.liveness_probe {
            let probe_desc = if probe.probe_type == "http" {
                format!(
                    "HTTP GET :{}{} every {}s (threshold={}, timeout={}s, delay={}s)",
                    probe.port,
                    probe.path,
                    probe.period_secs,
                    probe.failure_threshold,
                    probe.timeout_secs,
                    probe.initial_delay_secs
                )
            } else {
                format!(
                    "TCP :{} every {}s (threshold={}, timeout={}s, delay={}s)",
                    probe.port,
                    probe.period_secs,
                    probe.failure_threshold,
                    probe.timeout_secs,
                    probe.initial_delay_secs
                )
            };
            out.push_str(&format!("Probe:      {probe_desc}\n"));
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
    journal_client: &mut super::execute::AuthConfigClient,
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
    journal_client: &mut super::execute::AuthConfigClient,
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
    journal_client: &mut super::execute::AuthConfigClient,
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
    let mut lc_for_services: Option<lattice_client::LatticeClient> = None;
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
                lc_for_services = Some(lc);
            }
            Err(e) => {
                out.push_str(&format!("  Status: UNHEALTHY ({e})\n"));
                all_healthy = false;
            }
        },
    }

    // --- Service registry health (Feature 3) ---
    out.push_str("\n=== Lattice Services ===\n");
    match lc_for_services {
        None => out.push_str("  (unavailable — see above)\n"),
        Some(ref mut lc) => match lc.list_services().await {
            Ok(resp) => {
                if resp.names.is_empty() {
                    out.push_str("  Registered: 0 services\n");
                } else {
                    let mut total_endpoints = 0u32;
                    let mut service_lines = Vec::new();
                    for name in &resp.names {
                        match lc.lookup_service(name).await {
                            Ok(svc) => {
                                let count = svc.endpoints.len() as u32;
                                total_endpoints += count;
                                service_lines.push(format!("    {name}: {count} endpoints"));
                            }
                            Err(_) => {
                                service_lines.push(format!("    {name}: (lookup failed)"));
                            }
                        }
                    }
                    out.push_str(&format!(
                        "  Registered: {} services, {} endpoints\n",
                        resp.names.len(),
                        total_endpoints
                    ));
                    for line in &service_lines {
                        out.push_str(&format!("{line}\n"));
                    }
                }
            }
            Err(e) => out.push_str(&format!("  Service registry error: {e}\n")),
        },
    }

    out.push_str(&format!("\nOverall: {}", if all_healthy { "PASS" } else { "FAIL" }));

    Ok(out)
}

// ─── Services ─────────────────────────────────────────────

/// List registered services from the lattice service registry.
pub async fn list_services(config: &DelegationConfig) -> anyhow::Result<String> {
    let mut lc = connect_lattice(config).await?;

    let resp = lc.list_services().await.map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut out = String::new();
    if resp.names.is_empty() {
        out.push_str("No services registered.\n");
    } else {
        out.push_str(&format!("{} registered services:\n", resp.names.len()));
        for name in &resp.names {
            out.push_str(&format!("  {name}\n"));
        }
    }

    Ok(out)
}

/// Look up endpoints for a named service.
pub async fn lookup_service(config: &DelegationConfig, name: &str) -> anyhow::Result<String> {
    let mut lc = connect_lattice(config).await?;

    let resp = lc.lookup_service(name).await.map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut out = String::new();
    out.push_str(&format!("Service: {}\n", resp.name));

    if resp.endpoints.is_empty() {
        out.push_str("  No endpoints registered.\n");
    } else {
        out.push_str(&format!("  {} endpoints:\n", resp.endpoints.len()));
        for ep in &resp.endpoints {
            out.push_str(&format!(
                "    alloc={} tenant={} port={} proto={} nodes=[{}]\n",
                ep.allocation_id,
                ep.tenant,
                ep.port,
                if ep.protocol.is_empty() { "tcp" } else { &ep.protocol },
                ep.nodes.join(", ")
            ));
        }
    }

    Ok(out)
}

// ─── DAGs ─────────────────────────────────────────────────

/// List DAG workflows from lattice.
pub async fn list_dags(
    config: &DelegationConfig,
    tenant: Option<&str>,
    state: Option<&str>,
    limit: u32,
) -> anyhow::Result<String> {
    let mut lc = connect_lattice(config).await?;

    let req = lattice_client::proto::ListDagsRequest {
        tenant: tenant.unwrap_or_default().to_string(),
        state: state.unwrap_or_default().to_string(),
        limit,
        ..Default::default()
    };

    let resp = lc.list_dags(req).await.map_err(|e| anyhow::anyhow!("{e}"))?;

    if resp.dags.is_empty() {
        return Ok("No DAGs found.".to_string());
    }

    let mut out = String::new();
    out.push_str(&format!("{:<36}  {:<12}  {:<12}  {}\n", "DAG ID", "STATE", "STEPS", "CREATED"));
    out.push_str(&"-".repeat(80));
    out.push('\n');

    for dag in &resp.dags {
        let created =
            dag.created_at.as_ref().map_or_else(|| "-".into(), |t| format!("{}s", t.seconds));
        out.push_str(&format!(
            "{:<36}  {:<12}  {:<12}  {}\n",
            dag.dag_id,
            dag.state,
            dag.allocations.len(),
            created,
        ));
    }

    Ok(out)
}

/// Inspect a single DAG workflow.
pub async fn inspect_dag(config: &DelegationConfig, dag_id: &str) -> anyhow::Result<String> {
    let mut lc = connect_lattice(config).await?;

    let dag = lc.get_dag(dag_id).await.map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut out = String::new();
    out.push_str(&format!("DAG:       {}\n", dag.dag_id));
    out.push_str(&format!("State:     {}\n", dag.state));
    out.push_str(&format!("Steps:     {}\n", dag.allocations.len()));

    if let Some(ref ts) = dag.created_at {
        out.push_str(&format!("Created:   {}s\n", ts.seconds));
    }
    if let Some(ref ts) = dag.completed_at {
        out.push_str(&format!("Completed: {}s\n", ts.seconds));
    }

    if !dag.allocations.is_empty() {
        out.push_str("\nAllocations:\n");
        for alloc in &dag.allocations {
            out.push_str(&format!(
                "  {} — {} ({})\n",
                alloc.allocation_id, alloc.state, alloc.user,
            ));
        }
    }

    Ok(out)
}

/// Cancel a DAG workflow.
pub async fn cancel_dag(config: &DelegationConfig, dag_id: &str) -> anyhow::Result<String> {
    let mut lc = connect_lattice(config).await?;

    let resp = lc.cancel_dag(dag_id).await.map_err(|e| anyhow::anyhow!("{e}"))?;

    if resp.success {
        Ok(format!(
            "DAG {dag_id} cancelled ({} allocations cancelled).",
            resp.allocations_cancelled
        ))
    } else {
        Ok(format!("DAG {dag_id} cancel returned: success=false"))
    }
}

// ─── Budget ───────────────────────────────────────────────

/// Tenant budget/usage from lattice.
pub async fn tenant_budget(
    config: &DelegationConfig,
    tenant_id: &str,
    days: u32,
) -> anyhow::Result<String> {
    let mut lc = connect_lattice(config).await?;

    let resp = lc.tenant_usage(tenant_id, days).await.map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut out = String::new();
    out.push_str(&format!("Tenant: {}\n", resp.tenant));
    out.push_str(&format!(
        "Period: {} — {} ({} days)\n",
        resp.period_start, resp.period_end, resp.period_days
    ));
    out.push_str(&format!("GPU hours:  {:.1}", resp.gpu_hours_used));
    if let Some(budget) = resp.gpu_hours_budget {
        out.push_str(&format!(" / {budget:.1}"));
    }
    if let Some(frac) = resp.gpu_fraction_used {
        out.push_str(&format!(" ({:.1}%)", frac * 100.0));
    }
    out.push('\n');
    out.push_str(&format!("Node hours: {:.1}", resp.node_hours_used));
    if let Some(budget) = resp.node_hours_budget {
        out.push_str(&format!(" / {budget:.1}"));
    }
    if let Some(frac) = resp.node_fraction_used {
        out.push_str(&format!(" ({:.1}%)", frac * 100.0));
    }
    out.push('\n');

    Ok(out)
}

/// User budget/usage from lattice (across all tenants).
pub async fn user_budget(
    config: &DelegationConfig,
    user: &str,
    days: u32,
) -> anyhow::Result<String> {
    let mut lc = connect_lattice(config).await?;

    let resp = lc.user_usage(user, days).await.map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut out = String::new();
    out.push_str(&format!("User: {}\n", resp.user));
    out.push_str(&format!("Period: {} — {}\n", resp.period_start, resp.period_end));
    out.push_str(&format!("Total GPU hours: {:.1}\n", resp.total_gpu_hours));

    if !resp.tenants.is_empty() {
        out.push_str(&format!(
            "\n{:<16}  {:<14}  {:<14}  {}\n",
            "TENANT", "GPU HOURS", "GPU BUDGET", "NODE HOURS"
        ));
        out.push_str(&"-".repeat(65));
        out.push('\n');
        for t in &resp.tenants {
            let gpu_budget = t.gpu_hours_budget.map_or_else(|| "-".into(), |b| format!("{b:.1}"));
            out.push_str(&format!(
                "{:<16}  {:<14.1}  {:<14}  {:.1}\n",
                t.tenant, t.gpu_hours_used, gpu_budget, t.node_hours_used,
            ));
        }
    }

    Ok(out)
}

// ─── Backup ───────────────────────────────────────────────

/// Create a backup of lattice Raft state.
pub async fn backup_create(
    journal_client: &mut super::execute::AuthConfigClient,
    config: &DelegationConfig,
    path: &str,
    principal: &str,
    role: &str,
) -> anyhow::Result<String> {
    // Audit-log the backup operation
    let entry = pact_common::proto::config::ConfigEntry {
        sequence: 0,
        timestamp: None,
        entry_type: 12, // SERVICE_LIFECYCLE — admin delegation
        scope: Some(pact_common::proto::config::Scope {
            scope: Some(pact_common::proto::config::scope::Scope::Global(true)),
        }),
        author: Some(pact_common::proto::config::Identity {
            principal: principal.to_string(),
            principal_type: "Human".to_string(),
            role: role.to_string(),
        }),
        parent: None,
        state_delta: None,
        policy_ref: format!("delegate:lattice:backup_create:{path}"),
        ttl: None,
        emergency_reason: None,
    };
    let _audit = journal_client
        .append_entry(tonic::Request::new(pact_common::proto::journal::AppendEntryRequest {
            entry: Some(entry),
        }))
        .await
        .map_err(|e| anyhow::anyhow!("audit logging for backup_create failed: {e}"))?;

    let mut lc = connect_lattice(config).await?;
    let resp = lc.create_backup(path).await.map_err(|e| anyhow::anyhow!("{e}"))?;

    if resp.success {
        let mut out = String::new();
        out.push_str(&format!("Backup created: {}\n", resp.message));
        out.push_str(&format!(
            "  Nodes: {}, Allocations: {}, Tenants: {}, Audit entries: {}\n",
            resp.node_count, resp.allocation_count, resp.tenant_count, resp.audit_entry_count,
        ));
        if let Some(ref ts) = resp.backup_timestamp {
            out.push_str(&format!("  Timestamp: {}s\n", ts.seconds));
        }
        Ok(out)
    } else {
        Err(anyhow::anyhow!("Backup failed: {}", resp.message))
    }
}

/// Verify a lattice backup.
pub async fn backup_verify(config: &DelegationConfig, path: &str) -> anyhow::Result<String> {
    let mut lc = connect_lattice(config).await?;
    let resp = lc.verify_backup(path).await.map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut out = String::new();
    out.push_str(&format!(
        "Backup valid: {}\n{}\n",
        if resp.valid { "YES" } else { "NO" },
        resp.message,
    ));
    if let Some(ref ts) = resp.backup_timestamp {
        out.push_str(&format!("  Timestamp: {}s\n", ts.seconds));
    }
    out.push_str(&format!(
        "  Snapshot: term={}, index={}\n",
        resp.snapshot_term, resp.snapshot_index,
    ));

    Ok(out)
}

/// Restore lattice state from a backup. Requires --confirm.
pub async fn backup_restore(
    journal_client: &mut super::execute::AuthConfigClient,
    config: &DelegationConfig,
    path: &str,
    principal: &str,
    role: &str,
) -> anyhow::Result<String> {
    // Audit-log the restore operation
    let entry = pact_common::proto::config::ConfigEntry {
        sequence: 0,
        timestamp: None,
        entry_type: 12,
        scope: Some(pact_common::proto::config::Scope {
            scope: Some(pact_common::proto::config::scope::Scope::Global(true)),
        }),
        author: Some(pact_common::proto::config::Identity {
            principal: principal.to_string(),
            principal_type: "Human".to_string(),
            role: role.to_string(),
        }),
        parent: None,
        state_delta: None,
        policy_ref: format!("delegate:lattice:backup_restore:{path}"),
        ttl: None,
        emergency_reason: None,
    };
    let _audit = journal_client
        .append_entry(tonic::Request::new(pact_common::proto::journal::AppendEntryRequest {
            entry: Some(entry),
        }))
        .await
        .map_err(|e| anyhow::anyhow!("audit logging for backup_restore failed: {e}"))?;

    let mut lc = connect_lattice(config).await?;
    let resp = lc.restore_backup(path).await.map_err(|e| anyhow::anyhow!("{e}"))?;

    if resp.success {
        Ok(format!("Restore completed: {}", resp.message))
    } else {
        Err(anyhow::anyhow!("Restore failed: {}", resp.message))
    }
}

// ─── Node Query ───────────────────────────────────────────

/// List nodes from lattice with optional filters.
pub async fn list_lattice_nodes(
    config: &DelegationConfig,
    state: Option<&str>,
    vcluster: Option<&str>,
    limit: u32,
) -> anyhow::Result<String> {
    let mut lc = connect_lattice(config).await?;

    let req = lattice_client::proto::ListNodesRequest {
        state: state.unwrap_or_default().to_string(),
        vcluster: vcluster.unwrap_or_default().to_string(),
        limit,
        ..Default::default()
    };

    let resp = lc.list_nodes(req).await.map_err(|e| anyhow::anyhow!("{e}"))?;

    if resp.nodes.is_empty() {
        return Ok("No nodes found.".to_string());
    }

    let mut out = String::new();
    out.push_str(&format!(
        "{:<20}  {:<10}  {:<8}  {:<6}  {:<8}  {:<16}  {}\n",
        "NODE", "STATE", "GPUs", "CORES", "MEM GB", "VCLUSTER", "REASON"
    ));
    out.push_str(&"-".repeat(90));
    out.push('\n');

    for n in &resp.nodes {
        out.push_str(&format!(
            "{:<20}  {:<10}  {:<8}  {:<6}  {:<8}  {:<16}  {}\n",
            n.node_id,
            n.state,
            n.gpu_count,
            n.cpu_cores,
            n.memory_gb,
            if n.owner_vcluster.is_empty() { "-" } else { &n.owner_vcluster },
            n.state_reason,
        ));
    }

    Ok(out)
}

/// Inspect a single node from lattice.
pub async fn inspect_lattice_node(
    config: &DelegationConfig,
    node_id: &str,
) -> anyhow::Result<String> {
    let mut lc = connect_lattice(config).await?;

    let n = lc.get_node(node_id).await.map_err(|e| anyhow::anyhow!("{e}"))?;

    let mut out = String::new();
    out.push_str(&format!("Node:      {}\n", n.node_id));
    out.push_str(&format!("State:     {}\n", n.state));
    if !n.state_reason.is_empty() {
        out.push_str(&format!("Reason:    {}\n", n.state_reason));
    }
    out.push_str(&format!("Group:     {}\n", n.group));
    out.push_str(&format!("GPU:       {} x {}\n", n.gpu_count, n.gpu_type));
    out.push_str(&format!("CPU:       {} cores\n", n.cpu_cores));
    out.push_str(&format!("Memory:    {} GB\n", n.memory_gb));
    if !n.features.is_empty() {
        out.push_str(&format!("Features:  {}\n", n.features.join(", ")));
    }
    if !n.owner_tenant.is_empty() {
        out.push_str(&format!("Tenant:    {}\n", n.owner_tenant));
    }
    if !n.owner_vcluster.is_empty() {
        out.push_str(&format!("vCluster:  {}\n", n.owner_vcluster));
    }
    if !n.owner_allocation.is_empty() {
        out.push_str(&format!("Allocation: {}\n", n.owner_allocation));
    }
    if !n.allocation_ids.is_empty() {
        out.push_str(&format!("Active allocations: {}\n", n.allocation_ids.join(", ")));
    }
    if let Some(ref ts) = n.last_heartbeat {
        out.push_str(&format!("Heartbeat: {}s\n", ts.seconds));
    }

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

    #[tokio::test]
    async fn list_services_no_endpoint() {
        let config = DelegationConfig::default();
        let result = list_services(&config).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not configured"));
    }

    // --- New delegation function tests (no endpoint) ---

    #[tokio::test]
    async fn list_dags_no_endpoint() {
        let config = DelegationConfig::default();
        let result = list_dags(&config, None, None, 50).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not configured"));
    }

    #[tokio::test]
    async fn lookup_service_no_endpoint() {
        let config = DelegationConfig::default();
        let result = lookup_service(&config, "inference-api").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not configured"));
    }

    #[tokio::test]
    async fn inspect_dag_no_endpoint() {
        let config = DelegationConfig::default();
        let result = inspect_dag(&config, "dag-123").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not configured"));
    }

    #[tokio::test]
    async fn cancel_dag_no_endpoint() {
        let config = DelegationConfig::default();
        let result = cancel_dag(&config, "dag-123").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not configured"));
    }

    #[tokio::test]
    async fn tenant_budget_no_endpoint() {
        let config = DelegationConfig::default();
        let result = tenant_budget(&config, "ml-team", 90).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not configured"));
    }

    #[tokio::test]
    async fn user_budget_no_endpoint() {
        let config = DelegationConfig::default();
        let result = user_budget(&config, "alice", 90).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not configured"));
    }

    #[tokio::test]
    async fn backup_verify_no_endpoint() {
        let config = DelegationConfig::default();
        let result = backup_verify(&config, "/tmp/backup.bin").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not configured"));
    }

    #[tokio::test]
    async fn list_lattice_nodes_no_endpoint() {
        let config = DelegationConfig::default();
        let result = list_lattice_nodes(&config, None, None, 100).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not configured"));
    }

    #[tokio::test]
    async fn inspect_lattice_node_no_endpoint() {
        let config = DelegationConfig::default();
        let result = inspect_lattice_node(&config, "node-042").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not configured"));
    }
}
