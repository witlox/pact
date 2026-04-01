//! pact CLI — configuration management and admin operations for HPC/AI.
//!
//! See `docs/architecture/cli-design.md` for command reference.

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use pact_cli::commands::config::CliConfig;
use pact_cli::commands::execute;

use pact_common::config::DelegationConfig;

/// pact — promise-based config management for HPC/AI infrastructure.
#[derive(Parser, Debug)]
#[command(name = "pact", version, about)]
struct Cli {
    /// Journal endpoint (overrides config file and PACT_ENDPOINT).
    #[arg(long, global = true)]
    endpoint: Option<String>,

    /// OIDC bearer token (overrides config file and PACT_TOKEN).
    #[arg(long, global = true)]
    token: Option<String>,

    /// Default vCluster scope (overrides config file and PACT_VCLUSTER).
    #[arg(long, global = true)]
    vcluster: Option<String>,

    /// Output format: text (default) or json.
    #[arg(long, global = true, default_value = "text")]
    output: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Show node/vCluster state, drift, and capabilities.
    Status {
        /// Node ID to query (default: all nodes in vCluster).
        node: Option<String>,
        /// vCluster scope.
        #[arg(long)]
        vcluster: Option<String>,
    },

    /// Show declared vs actual state differences.
    Diff {
        /// Node ID to diff.
        node: Option<String>,
        /// Show committed node deltas not yet promoted to overlay.
        #[arg(long)]
        committed: bool,
    },

    /// Show configuration history.
    Log {
        /// Number of entries to show.
        #[arg(short, long, default_value = "20")]
        n: u32,
        /// Scope filter (node:X, vc:X, or global).
        #[arg(long)]
        scope: Option<String>,
    },

    /// Commit drift on current node as a configuration entry.
    Commit {
        /// Commit message.
        #[arg(short, long)]
        m: String,
    },

    /// Roll back to a previous configuration state.
    Rollback {
        /// Target sequence number to roll back to.
        seq: u64,
    },

    /// Run a command on a remote node (whitelisted).
    Exec {
        /// Target node ID.
        node: String,
        /// Command and arguments (after --).
        #[arg(last = true)]
        command: Vec<String>,
    },

    /// Open an interactive shell session on a node.
    Shell {
        /// Target node ID.
        node: String,
    },

    /// Enter or exit emergency mode.
    Emergency {
        #[command(subcommand)]
        action: EmergencySubcommand,
    },

    /// Manage two-person approval workflow.
    Approve {
        #[command(subcommand)]
        action: ApproveSubcommand,
    },

    /// Service management.
    Service {
        #[command(subcommand)]
        action: ServiceSubcommand,
    },

    /// Show node capability report.
    Cap {
        /// Node ID.
        node: Option<String>,
    },

    /// Live event stream.
    Watch {
        /// vCluster scope.
        #[arg(long)]
        vcluster: Option<String>,
    },

    /// Apply a declarative config spec.
    Apply {
        /// Path to TOML spec file.
        spec: String,
    },

    /// Node enrollment and management.
    Node {
        #[command(subcommand)]
        action: NodeSubcommand,
    },

    /// Promote committed node deltas to vCluster overlay.
    Promote {
        /// Node ID to promote deltas from.
        node: String,
        /// Preview changes without applying.
        #[arg(long)]
        dry_run: bool,
    },

    /// Drain workloads from a node (delegates to lattice).
    Drain {
        /// Target node ID.
        node: String,
    },

    /// Remove node from scheduling (delegates to lattice).
    Cordon {
        /// Target node ID.
        node: String,
    },

    /// Return node to scheduling (delegates to lattice).
    Uncordon {
        /// Target node ID.
        node: String,
    },

    /// Cancel a drain, returning node to Ready (delegates to lattice).
    Undrain {
        /// Target node ID.
        node: String,
    },

    /// Reboot a node via BMC (delegates to OpenCHAMI).
    Reboot {
        /// Target node ID.
        node: String,
    },

    /// Re-image a node (delegates to OpenCHAMI).
    Reimage {
        /// Target node ID.
        node: String,
    },

    /// vCluster group management.
    Group {
        #[command(subcommand)]
        action: GroupSubcommand,
    },

    /// Retrieve diagnostic logs from nodes.
    Diag {
        /// Node ID (omit for fleet-wide with --vcluster).
        node: Option<String>,
        /// Number of lines per source (default: 100).
        #[arg(long, default_value = "100")]
        lines: u32,
        /// Source filter: system, service, or all (default: all).
        #[arg(long, default_value = "all")]
        source: String,
        /// Specific service name.
        #[arg(long)]
        service: Option<String>,
        /// Server-side grep pattern.
        #[arg(long)]
        grep: Option<String>,
        /// vCluster for fleet-wide query.
        #[arg(long)]
        vcluster: Option<String>,
    },

    /// Manage drift detection blacklist.
    Blacklist {
        #[command(subcommand)]
        action: BlacklistSubcommand,
    },

    /// Job/allocation management (lattice).
    Jobs {
        #[command(subcommand)]
        action: JobsSubcommand,
    },

    /// Show scheduling queue status (lattice).
    Queue {
        /// vCluster to query.
        #[arg(long)]
        vcluster: Option<String>,
    },

    /// Combined cluster status (pact + lattice).
    #[command(name = "cluster")]
    ClusterStatus,

    /// Query audit logs (pact, lattice, or both).
    Audit {
        /// Source: pact, lattice, or all (default: all).
        #[arg(long, default_value = "all")]
        source: String,
        /// Number of entries.
        #[arg(short, long, default_value = "20")]
        n: u32,
    },

    /// Resource accounting (lattice).
    Accounting {
        /// vCluster filter.
        #[arg(long)]
        vcluster: Option<String>,
    },

    /// DAG workflow management (lattice).
    Dag {
        #[command(subcommand)]
        action: DagSubcommand,
    },

    /// Budget/usage tracking (lattice).
    Budget {
        #[command(subcommand)]
        action: BudgetSubcommand,
    },

    /// Lattice backup/restore operations.
    Backup {
        #[command(subcommand)]
        action: BackupSubcommand,
    },

    /// Query lattice node state.
    Nodes {
        #[command(subcommand)]
        action: NodesSubcommand,
    },

    /// Combined system health check (pact + lattice).
    Health,

    /// Service registry operations (lattice).
    Services {
        #[command(subcommand)]
        action: ServicesSubcommand,
    },

    /// Extend commit window.
    Extend {
        /// Additional minutes (default: 15).
        #[arg(default_value = "15")]
        mins: u32,
    },

    /// Authenticate with the pact-journal server.
    Login {
        /// Server URL (overrides default).
        #[arg(long)]
        server: Option<String>,
        /// Force device code flow (headless environments).
        #[arg(long)]
        device_code: bool,
        /// Use service account (client credentials) flow.
        #[arg(long)]
        service_account: bool,
    },

    /// End the current session.
    Logout,
}

#[derive(Subcommand, Debug)]
enum EmergencySubcommand {
    /// Enter emergency mode.
    Start {
        /// Reason for emergency.
        #[arg(short, long)]
        reason: String,
    },
    /// Exit emergency mode.
    End {
        /// Force-end another admin's emergency.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand, Debug)]
enum ApproveSubcommand {
    /// List pending approval requests.
    List,
    /// Approve a pending request.
    Accept {
        /// Approval ID.
        id: String,
    },
    /// Deny a pending request.
    Deny {
        /// Approval ID.
        id: String,
        /// Denial reason.
        #[arg(short, long)]
        m: String,
    },
}

#[derive(Subcommand, Debug)]
enum ServiceSubcommand {
    /// Show service status.
    Status {
        /// Service name (or all).
        name: Option<String>,
    },
    /// Restart a service.
    Restart {
        /// Service name.
        name: String,
    },
    /// Stream service logs.
    Logs {
        /// Service name.
        name: String,
    },
}

#[derive(Subcommand, Debug)]
enum NodeSubcommand {
    /// Enroll (register) a node.
    Enroll {
        /// Node ID.
        node_id: String,
        /// Primary MAC address.
        #[arg(long)]
        mac: String,
        /// BMC serial number.
        #[arg(long)]
        bmc_serial: Option<String>,
    },
    /// Decommission a node.
    Decommission {
        /// Node ID.
        node_id: String,
        /// Force decommission even with active sessions.
        #[arg(long)]
        force: bool,
    },
    /// Assign a node to a vCluster.
    Assign {
        /// Node ID.
        node_id: String,
        /// Target vCluster.
        #[arg(long)]
        vcluster: String,
    },
    /// Unassign a node from its vCluster.
    Unassign {
        /// Node ID.
        node_id: String,
    },
    /// Move a node between vClusters.
    Move {
        /// Node ID.
        node_id: String,
        /// Target vCluster.
        #[arg(long)]
        to_vcluster: String,
    },
    /// List enrolled nodes.
    List {
        /// Filter by enrollment state (e.g., active, inactive, registered, revoked).
        #[arg(long)]
        state: Option<String>,
        /// Filter by vCluster.
        #[arg(long)]
        vcluster: Option<String>,
        /// Show only unassigned nodes.
        #[arg(long)]
        unassigned: bool,
    },
    /// Inspect a node's enrollment details.
    Inspect {
        /// Node ID.
        node_id: String,
    },
    /// Import nodes from OpenCHAMI SMD inventory.
    Import {
        /// Filter by SMD group/role.
        #[arg(long)]
        group: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum GroupSubcommand {
    /// List all vClusters.
    List,
    /// Show vCluster details.
    Show {
        /// vCluster name.
        name: String,
    },
    /// Update vCluster policy.
    SetPolicy {
        /// vCluster name.
        name: String,
        /// Path to policy TOML file.
        policy: String,
    },
}

#[derive(Subcommand, Debug)]
enum JobsSubcommand {
    /// List running jobs.
    List {
        /// Filter by node.
        #[arg(long)]
        node: Option<String>,
        /// Filter by vCluster.
        #[arg(long)]
        vcluster: Option<String>,
    },
    /// Cancel a running job.
    Cancel {
        /// Job/allocation ID.
        id: String,
    },
    /// Inspect job details.
    Inspect {
        /// Job/allocation ID.
        id: String,
    },
}

#[derive(Subcommand, Debug)]
enum ServicesSubcommand {
    /// List registered services.
    List,
    /// Look up endpoints for a service.
    Lookup {
        /// Service name.
        name: String,
    },
}

#[derive(Subcommand, Debug)]
enum BlacklistSubcommand {
    /// List current blacklist entries.
    List,
    /// Add a path pattern to the blacklist.
    Add {
        /// Glob pattern (e.g., "/custom/path/**").
        pattern: String,
    },
    /// Remove a path pattern from the blacklist.
    Remove {
        /// Glob pattern to remove.
        pattern: String,
    },
}

#[derive(Subcommand, Debug)]
enum DagSubcommand {
    /// List DAG workflows.
    List {
        /// Filter by tenant.
        #[arg(long)]
        tenant: Option<String>,
        /// Filter by state (running, completed, failed, cancelled).
        #[arg(long)]
        state: Option<String>,
        /// Max results.
        #[arg(short, long, default_value = "50")]
        n: u32,
    },
    /// Inspect a DAG workflow.
    Inspect {
        /// DAG ID.
        id: String,
    },
    /// Cancel a DAG workflow.
    Cancel {
        /// DAG ID.
        id: String,
    },
}

#[derive(Subcommand, Debug)]
enum BudgetSubcommand {
    /// Show tenant budget/usage.
    Tenant {
        /// Tenant ID (maps to vCluster).
        id: String,
        /// Rolling window in days (default: 90).
        #[arg(long, default_value = "90")]
        days: u32,
    },
    /// Show user budget/usage across all tenants.
    User {
        /// User ID.
        id: String,
        /// Rolling window in days (default: 90).
        #[arg(long, default_value = "90")]
        days: u32,
    },
}

#[derive(Subcommand, Debug)]
enum BackupSubcommand {
    /// Create a backup of lattice Raft state.
    Create {
        /// Backup file path.
        path: String,
    },
    /// Verify a backup file.
    Verify {
        /// Backup file path.
        path: String,
    },
    /// Restore lattice state from a backup.
    Restore {
        /// Backup file path.
        path: String,
        /// Required confirmation flag.
        #[arg(long)]
        confirm: bool,
    },
}

#[derive(Subcommand, Debug)]
enum NodesSubcommand {
    /// List lattice nodes.
    List {
        /// Filter by state (ready, draining, drained, down, etc.).
        #[arg(long)]
        state: Option<String>,
        /// Filter by vCluster.
        #[arg(long)]
        vcluster: Option<String>,
        /// Max results.
        #[arg(short, long, default_value = "100")]
        n: u32,
    },
    /// Inspect a lattice node.
    Inspect {
        /// Node ID.
        node_id: String,
    },
}

#[tokio::main]
#[allow(clippy::too_many_lines, clippy::large_stack_frames)]
async fn main() {
    // Hide lattice-only commands when PACT_LATTICE_ENDPOINT is not configured.
    // Commands still compile and work if invoked directly — they just don't
    // clutter --help for sites without lattice.
    let cli = if std::env::var("PACT_LATTICE_ENDPOINT").is_err() {
        let lattice_commands = [
            "drain",
            "undrain",
            "cordon",
            "uncordon",
            "jobs",
            "queue",
            "accounting",
            "dag",
            "budget",
            "backup",
            "nodes",
            "services",
        ];
        let mut cmd = Cli::command();
        for name in &lattice_commands {
            cmd = cmd.mut_subcommand(name, |sub| sub.hide(true));
        }
        Cli::from_arg_matches(&cmd.get_matches()).unwrap_or_else(|e| e.exit())
    } else {
        Cli::parse()
    };

    // Load config with precedence: CLI args > env vars > config file > defaults
    let mut config = CliConfig::load().with_env_overrides();

    if let Some(endpoint) = cli.endpoint {
        config.endpoint = endpoint;
    }
    if let Some(token) = cli.token {
        config.token = Some(token);
    }
    if let Some(vcluster) = cli.vcluster {
        config.default_vcluster = Some(vcluster);
    }

    // Initialize tracing (only WARN+ unless RUST_LOG is set)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::WARN.into()),
        )
        .init();

    // Commands that need the journal gRPC client
    let needs_journal = matches!(
        cli.command,
        Commands::Status { .. }
            | Commands::Log { .. }
            | Commands::Commit { .. }
            | Commands::Rollback { .. }
            | Commands::Diff { .. }
            | Commands::Emergency { .. }
            | Commands::Approve { .. }
            | Commands::Watch { .. }
            | Commands::Apply { .. }
            | Commands::Exec { .. }
            | Commands::Service { .. }
            | Commands::Cap { .. }
            | Commands::Extend { .. }
            | Commands::Node { .. }
            | Commands::Promote { .. }
            | Commands::Group { .. }
            | Commands::Blacklist { .. }
            | Commands::Diag { .. }
            | Commands::Drain { .. }
            | Commands::Cordon { .. }
            | Commands::Uncordon { .. }
            | Commands::Undrain { .. }
            | Commands::Reboot { .. }
            | Commands::Reimage { .. }
            | Commands::ClusterStatus
            | Commands::Audit { .. }
            | Commands::Backup { .. }
            | Commands::Health
            | Commands::Services { .. }
    );

    // Resolve auth token via hpc-auth (cached token from `pact login`).
    // Priority: --token CLI arg > PACT_TOKEN env > hpc-auth cache.
    // Unauthenticated commands (login, logout, version, help) skip this.
    let token = if needs_journal {
        if let Ok(t) = config.resolve_token() {
            t
        } else {
            // No manual token — try hpc-auth cache
            let auth = hpc_auth::AuthClient::new(hpc_auth::AuthClientConfig {
                server_url: config.endpoint.clone(),
                app_name: "pact".to_string(),
                permission_mode: hpc_auth::PermissionMode::Strict,
                idp_override: None,
                flow_override: None,
                timeout: std::time::Duration::from_secs(30),
            });
            match auth.get_token().await {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("Error: not authenticated. Run `pact login` first.");
                    eprintln!("  Detail: {e}");
                    std::process::exit(2);
                }
            }
        }
    } else {
        String::new()
    };

    // Create authenticated journal channel (P1: token injected on all RPCs)
    let auth_channel = if needs_journal {
        match execute::connect_authenticated(&config, token.clone()).await {
            Ok(ch) => Some(ch),
            Err(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
    } else {
        None
    };
    let mut journal_client =
        auth_channel.as_ref().map(pact_cli::commands::execute::AuthenticatedChannel::config_client);

    // Load delegation config from env vars (lattice + node management endpoints).
    // Lattice token falls back to the pact auth token — in production, the admin
    // logs in once via `pact login` and the same OIDC token is reused for lattice
    // delegation (both systems share the same IdP).
    let node_mgmt_backend = std::env::var("PACT_NODE_MGMT_BACKEND")
        .ok()
        .and_then(|v| serde_json::from_value(serde_json::Value::String(v)).ok());
    let delegation_config = DelegationConfig {
        lattice_endpoint: std::env::var("PACT_LATTICE_ENDPOINT").ok(),
        lattice_token: std::env::var("PACT_LATTICE_TOKEN").ok().or_else(|| {
            if token.is_empty() {
                None
            } else {
                Some(token.clone())
            }
        }),
        node_mgmt_backend,
        node_mgmt_base_url: std::env::var("PACT_NODE_MGMT_URL").ok(),
        node_mgmt_token: std::env::var("PACT_NODE_MGMT_TOKEN").ok(),
        openchami_smd_url: std::env::var("PACT_OPENCHAMI_SMD_URL").ok(),
        openchami_token: std::env::var("PACT_OPENCHAMI_TOKEN").ok(),
        timeout_secs: 30,
    };

    let (principal, role) = execute::resolve_identity_from_token(&token);

    let result = match cli.command {
        Commands::Status { node, .. } => {
            let node_id = node.unwrap_or_else(|| "local".to_string());
            execute::status(journal_client.as_mut().unwrap(), &node_id).await
        }
        Commands::Log { n, scope } => {
            execute::log(journal_client.as_mut().unwrap(), n, scope.as_deref()).await
        }
        Commands::Commit { m } => {
            let vcluster = config.default_vcluster.as_deref().unwrap_or("default").to_string();
            execute::commit(journal_client.as_mut().unwrap(), &m, &vcluster, &principal, &role)
                .await
        }
        Commands::Rollback { seq } => {
            let vcluster = config.default_vcluster.as_deref().unwrap_or("default").to_string();
            execute::rollback(journal_client.as_mut().unwrap(), seq, &vcluster, &principal, &role)
                .await
        }
        Commands::Diff { node, committed: _ } => {
            // Diff queries entries, not a dedicated RPC
            let scope_filter = node.as_deref().map(|n| format!("node:{n}"));
            execute::log(journal_client.as_mut().unwrap(), 50, scope_filter.as_deref()).await
        }

        // Commands that need agent gRPC
        Commands::Exec { node, command } => {
            match pact_cli::commands::exec::parse_exec_command(&command) {
                Ok((cmd, args)) => {
                    match execute::resolve_agent_address(&node, auth_channel.as_ref().unwrap())
                        .await
                    {
                        Ok(agent_addr) => match execute::connect_agent(&agent_addr).await {
                            Ok(channel) => execute::exec_remote(channel, &token, &cmd, &args).await,
                            Err(e) => Err(e),
                        },
                        Err(e) => Err(e),
                    }
                }
                Err(e) => Err(anyhow::anyhow!("{e}")),
            }
        }
        Commands::Shell { node } => {
            let agent_addr =
                match execute::resolve_agent_address(&node, auth_channel.as_ref().unwrap()).await {
                    Ok(addr) => addr,
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                };
            match execute::connect_agent(&agent_addr).await {
                Ok(channel) => execute::shell_interactive(channel, &token).await,
                Err(e) => Err(e),
            }
        }
        Commands::Emergency { action } => {
            let vcluster = config.default_vcluster.as_deref().unwrap_or("default").to_string();
            match action {
                EmergencySubcommand::Start { reason } => {
                    execute::emergency_start(
                        journal_client.as_mut().unwrap(),
                        &reason,
                        &vcluster,
                        "cli-user",
                        "pact-platform-admin",
                    )
                    .await
                }
                EmergencySubcommand::End { force: _ } => {
                    execute::emergency_end(
                        journal_client.as_mut().unwrap(),
                        &vcluster,
                        "cli-user",
                        "pact-platform-admin",
                    )
                    .await
                }
            }
        }
        Commands::Approve { action } => {
            let ac = auth_channel.as_ref().unwrap();
            match action {
                ApproveSubcommand::List => execute::approve_list(ac, None).await,
                ApproveSubcommand::Accept { id } => {
                    execute::approve_decide(
                        ac,
                        &id,
                        "approved",
                        "cli-user",
                        "pact-platform-admin",
                        None,
                    )
                    .await
                }
                ApproveSubcommand::Deny { id, m } => {
                    execute::approve_decide(
                        ac,
                        &id,
                        "rejected",
                        "cli-user",
                        "pact-platform-admin",
                        Some(&m),
                    )
                    .await
                }
            }
        }
        Commands::Service { action } => {
            // Service commands delegate to agent exec with systemctl/journalctl
            let agent_addr =
                match execute::resolve_agent_address("local", auth_channel.as_ref().unwrap()).await
                {
                    Ok(addr) => addr,
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                };
            match execute::connect_agent(&agent_addr).await {
                Ok(channel) => match action {
                    ServiceSubcommand::Status { name } => {
                        let svc = name.as_deref().unwrap_or("--all");
                        execute::exec_remote(
                            channel,
                            &token,
                            "systemctl",
                            &["status".into(), svc.into()],
                        )
                        .await
                    }
                    ServiceSubcommand::Restart { name } => {
                        execute::exec_remote(
                            channel,
                            &token,
                            "systemctl",
                            &["restart".into(), name],
                        )
                        .await
                    }
                    ServiceSubcommand::Logs { name } => {
                        execute::exec_remote(
                            channel,
                            &token,
                            "journalctl",
                            &["-u".into(), name, "-n".into(), "50".into()],
                        )
                        .await
                    }
                },
                Err(e) => Err(e),
            }
        }
        Commands::Cap { node } => {
            // Cap queries agent's list of capabilities via ListCommands
            let node_id = node.as_deref().unwrap_or("local");
            let agent_addr =
                match execute::resolve_agent_address(node_id, auth_channel.as_ref().unwrap()).await
                {
                    Ok(addr) => addr,
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                };
            match execute::connect_agent(&agent_addr).await {
                Ok(channel) => execute::list_agent_commands(channel, &token).await,
                Err(e) => Err(e),
            }
        }
        Commands::Watch { vcluster } => {
            let vc =
                vcluster.as_deref().or(config.default_vcluster.as_deref()).unwrap_or("default");
            execute::watch(auth_channel.as_ref().unwrap(), vc).await
        }
        Commands::Apply { spec } => {
            execute::apply(journal_client.as_mut().unwrap(), &spec, &principal, &role).await
        }
        Commands::Extend { mins } => {
            let agent_addr =
                match execute::resolve_agent_address("local", auth_channel.as_ref().unwrap()).await
                {
                    Ok(addr) => addr,
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                };
            match execute::connect_agent(&agent_addr).await {
                Ok(channel) => execute::extend(channel, &token, mins).await,
                Err(e) => Err(e),
            }
        }
        Commands::Node { action } => {
            let ac = auth_channel.as_ref().unwrap();
            match action {
                NodeSubcommand::Enroll { node_id, mac, bmc_serial } => {
                    pact_cli::commands::node::enroll(ac, &node_id, &mac, bmc_serial.as_deref())
                        .await
                }
                NodeSubcommand::Decommission { node_id, force } => {
                    pact_cli::commands::node::decommission(ac, &node_id, force).await
                }
                NodeSubcommand::Assign { node_id, vcluster } => {
                    pact_cli::commands::node::assign(ac, &node_id, &vcluster).await
                }
                NodeSubcommand::Unassign { node_id } => {
                    pact_cli::commands::node::unassign(ac, &node_id).await
                }
                NodeSubcommand::Move { node_id, to_vcluster } => {
                    pact_cli::commands::node::move_node(ac, &node_id, &to_vcluster).await
                }
                NodeSubcommand::List { state, vcluster, unassigned } => {
                    pact_cli::commands::node::list(
                        ac,
                        state.as_deref(),
                        vcluster.as_deref(),
                        unassigned,
                    )
                    .await
                }
                NodeSubcommand::Inspect { node_id } => {
                    pact_cli::commands::node::inspect(ac, &node_id).await
                }
                NodeSubcommand::Import { group } => {
                    let Some(ref smd_url) = delegation_config.openchami_smd_url else {
                        eprintln!("Error: PACT_OPENCHAMI_SMD_URL not configured");
                        std::process::exit(1);
                    };
                    pact_cli::commands::node::import_from_smd(
                        ac,
                        smd_url,
                        delegation_config.openchami_token.as_deref(),
                        group.as_deref(),
                        delegation_config.timeout_secs,
                    )
                    .await
                }
            }
        }
        Commands::Promote { node, dry_run } => {
            pact_cli::commands::execute::promote_node(
                journal_client.as_mut().unwrap(),
                &node,
                dry_run,
            )
            .await
        }
        Commands::Drain { node } => {
            let result = pact_cli::commands::delegate::drain_node(
                journal_client.as_mut().unwrap(),
                &node,
                &principal,
                &role,
                &delegation_config,
            )
            .await;
            println!("{}", pact_cli::commands::delegate::format_delegation_result(&result));
            if !result.success {
                std::process::exit(1);
            }
            Ok(String::new())
        }
        Commands::Cordon { node } => {
            let result = pact_cli::commands::delegate::cordon_node(
                journal_client.as_mut().unwrap(),
                &node,
                &principal,
                &role,
                &delegation_config,
            )
            .await;
            println!("{}", pact_cli::commands::delegate::format_delegation_result(&result));
            if !result.success {
                std::process::exit(1);
            }
            Ok(String::new())
        }
        Commands::Uncordon { node } => {
            let result = pact_cli::commands::delegate::uncordon_node(
                journal_client.as_mut().unwrap(),
                &node,
                &principal,
                &role,
                &delegation_config,
            )
            .await;
            println!("{}", pact_cli::commands::delegate::format_delegation_result(&result));
            if !result.success {
                std::process::exit(1);
            }
            Ok(String::new())
        }
        Commands::Undrain { node } => {
            let result = pact_cli::commands::delegate::undrain_node(
                journal_client.as_mut().unwrap(),
                &node,
                &principal,
                &role,
                &delegation_config,
            )
            .await;
            println!("{}", pact_cli::commands::delegate::format_delegation_result(&result));
            if !result.success {
                std::process::exit(1);
            }
            Ok(String::new())
        }
        Commands::Reboot { node } => {
            let result = pact_cli::commands::delegate::reboot_node(
                journal_client.as_mut().unwrap(),
                &node,
                &principal,
                &role,
                &delegation_config,
            )
            .await;
            println!("{}", pact_cli::commands::delegate::format_delegation_result(&result));
            if !result.success {
                std::process::exit(1);
            }
            Ok(String::new())
        }
        Commands::Reimage { node } => {
            let result = pact_cli::commands::delegate::reimage_node(
                journal_client.as_mut().unwrap(),
                &node,
                &principal,
                &role,
                &delegation_config,
            )
            .await;
            println!("{}", pact_cli::commands::delegate::format_delegation_result(&result));
            if !result.success {
                std::process::exit(1);
            }
            Ok(String::new())
        }
        Commands::Group { action } => {
            let ac = auth_channel.as_ref().unwrap();
            match action {
                GroupSubcommand::List => execute::group_list(ac).await,
                GroupSubcommand::Show { name } => execute::group_show(ac, &name).await,
                GroupSubcommand::SetPolicy { name, policy } => {
                    execute::group_set_policy(ac, &name, &policy, &principal, &role).await
                }
            }
        }
        Commands::Diag { node, lines, source, service, grep, vcluster } => {
            if let Some(node_id) = node {
                // Single-node diag
                let agent_addr =
                    match execute::resolve_agent_address(&node_id, auth_channel.as_ref().unwrap())
                        .await
                    {
                        Ok(addr) => addr,
                        Err(e) => {
                            eprintln!("Error: {e}");
                            std::process::exit(1);
                        }
                    };
                match execute::connect_agent(&agent_addr).await {
                    Ok(channel) => {
                        pact_cli::commands::diag::diag_node(
                            channel,
                            &token,
                            &source,
                            service.as_deref(),
                            grep.as_deref(),
                            lines,
                        )
                        .await
                    }
                    Err(e) => Err(e),
                }
            } else if let Some(vc) = vcluster.as_deref().or(config.default_vcluster.as_deref()) {
                // Fleet-wide diag
                pact_cli::commands::diag::diag_fleet(
                    auth_channel.as_ref().unwrap().channel(),
                    &token,
                    vc,
                    &source,
                    service.as_deref(),
                    grep.as_deref(),
                    lines,
                )
                .await
            } else {
                Err(anyhow::anyhow!(
                    "specify --node <id> for single-node or --vcluster <name> for fleet-wide diag"
                ))
            }
        }
        Commands::Blacklist { action } => match action {
            BlacklistSubcommand::List => {
                let result = pact_cli::commands::blacklist::BlacklistResult {
                    operation: pact_cli::commands::blacklist::BlacklistOp::List,
                    paths: pact_cli::commands::blacklist::default_blacklist(),
                };
                println!("{}", pact_cli::commands::blacklist::format_blacklist_result(&result));
                Ok(String::new())
            }
            BlacklistSubcommand::Add { pattern } => {
                let vcluster = config.default_vcluster.as_deref().unwrap_or("default").to_string();
                execute::blacklist_add(
                    journal_client.as_mut().unwrap(),
                    &pattern,
                    &vcluster,
                    &principal,
                    &role,
                )
                .await
            }
            BlacklistSubcommand::Remove { pattern } => {
                let vcluster = config.default_vcluster.as_deref().unwrap_or("default").to_string();
                execute::blacklist_remove(
                    journal_client.as_mut().unwrap(),
                    &pattern,
                    &vcluster,
                    &principal,
                    &role,
                )
                .await
            }
        },
        Commands::Jobs { action } => match action {
            JobsSubcommand::List { node, vcluster } => {
                pact_cli::commands::lattice::list_jobs(
                    &delegation_config,
                    node.as_deref(),
                    vcluster.as_deref(),
                )
                .await
            }
            JobsSubcommand::Cancel { id } => {
                pact_cli::commands::lattice::cancel_job(&delegation_config, &id).await
            }
            JobsSubcommand::Inspect { id } => {
                pact_cli::commands::lattice::inspect_job(&delegation_config, &id).await
            }
        },
        Commands::Queue { vcluster } => {
            pact_cli::commands::lattice::queue_status(&delegation_config, vcluster.as_deref()).await
        }
        Commands::ClusterStatus => {
            pact_cli::commands::lattice::cluster_status(
                journal_client.as_mut().unwrap(),
                &delegation_config,
            )
            .await
        }
        Commands::Audit { source, n } => {
            pact_cli::commands::lattice::audit_combined(
                journal_client.as_mut().unwrap(),
                &delegation_config,
                &source,
                n,
            )
            .await
        }
        Commands::Accounting { vcluster } => {
            pact_cli::commands::lattice::accounting(&delegation_config, vcluster.as_deref()).await
        }
        Commands::Dag { action } => match action {
            DagSubcommand::List { tenant, state, n } => {
                pact_cli::commands::lattice::list_dags(
                    &delegation_config,
                    tenant.as_deref(),
                    state.as_deref(),
                    n,
                )
                .await
            }
            DagSubcommand::Inspect { id } => {
                pact_cli::commands::lattice::inspect_dag(&delegation_config, &id).await
            }
            DagSubcommand::Cancel { id } => {
                pact_cli::commands::lattice::cancel_dag(&delegation_config, &id).await
            }
        },
        Commands::Budget { action } => match action {
            BudgetSubcommand::Tenant { id, days } => {
                pact_cli::commands::lattice::tenant_budget(&delegation_config, &id, days).await
            }
            BudgetSubcommand::User { id, days } => {
                pact_cli::commands::lattice::user_budget(&delegation_config, &id, days).await
            }
        },
        Commands::Backup { action } => match action {
            BackupSubcommand::Create { path } => {
                pact_cli::commands::lattice::backup_create(
                    journal_client.as_mut().unwrap(),
                    &delegation_config,
                    &path,
                    &principal,
                    &role,
                )
                .await
            }
            BackupSubcommand::Verify { path } => {
                pact_cli::commands::lattice::backup_verify(&delegation_config, &path).await
            }
            BackupSubcommand::Restore { path, confirm } => {
                if confirm {
                    pact_cli::commands::lattice::backup_restore(
                        journal_client.as_mut().unwrap(),
                        &delegation_config,
                        &path,
                        &principal,
                        &role,
                    )
                    .await
                } else {
                    Err(anyhow::anyhow!(
                        "backup restore is destructive — pass --confirm to proceed"
                    ))
                }
            }
        },
        Commands::Nodes { action } => match action {
            NodesSubcommand::List { state, vcluster, n } => {
                pact_cli::commands::lattice::list_lattice_nodes(
                    &delegation_config,
                    state.as_deref(),
                    vcluster.as_deref(),
                    n,
                )
                .await
            }
            NodesSubcommand::Inspect { node_id } => {
                pact_cli::commands::lattice::inspect_lattice_node(&delegation_config, &node_id)
                    .await
            }
        },
        Commands::Health => {
            pact_cli::commands::lattice::health_check(
                journal_client.as_mut().unwrap(),
                &delegation_config,
            )
            .await
        }
        Commands::Services { action } => match action {
            ServicesSubcommand::List => {
                pact_cli::commands::lattice::list_services(&delegation_config).await
            }
            ServicesSubcommand::Lookup { name } => {
                pact_cli::commands::lattice::lookup_service(&delegation_config, &name).await
            }
        },
        Commands::Login { server, device_code, service_account } => {
            let server_url = server.unwrap_or_else(|| config.endpoint.clone());
            let flow_override = if device_code {
                Some(hpc_auth::OAuthFlow::DeviceCode)
            } else if service_account {
                Some(hpc_auth::OAuthFlow::ClientCredentials {
                    client_id: std::env::var("PACT_CLIENT_ID").unwrap_or_default(),
                    client_secret: std::env::var("PACT_CLIENT_SECRET").unwrap_or_default(),
                })
            } else {
                None
            };
            let auth = hpc_auth::AuthClient::new(hpc_auth::AuthClientConfig {
                server_url,
                app_name: "pact".to_string(),
                permission_mode: hpc_auth::PermissionMode::Strict,
                idp_override: None,
                flow_override,
                timeout: std::time::Duration::from_secs(30),
            });
            match auth.login().await {
                Ok(_) => Ok("Login successful.".to_string()),
                Err(e) => Err(anyhow::anyhow!("{e}")),
            }
        }
        Commands::Logout => {
            let auth = hpc_auth::AuthClient::new(hpc_auth::AuthClientConfig {
                server_url: config.endpoint.clone(),
                app_name: "pact".to_string(),
                permission_mode: hpc_auth::PermissionMode::Strict,
                idp_override: None,
                flow_override: None,
                timeout: std::time::Duration::from_secs(30),
            });
            match auth.logout().await {
                Ok(()) => Ok("Logged out.".to_string()),
                Err(e) => Err(anyhow::anyhow!("{e}")),
            }
        }
    };

    match result {
        Ok(output) => println!("{output}"),
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}
