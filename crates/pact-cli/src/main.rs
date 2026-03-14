//! pact CLI — configuration management and admin operations for HPC/AI.
//!
//! See `docs/architecture/cli-design.md` for command reference.

use clap::{Parser, Subcommand};
use pact_cli::commands::config::CliConfig;
use pact_cli::commands::execute;

use pact_common::proto::journal::config_service_client::ConfigServiceClient;

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

    /// Extend commit window.
    Extend {
        /// Additional minutes (default: 15).
        #[arg(default_value = "15")]
        mins: u32,
    },
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

#[tokio::main]
#[allow(clippy::too_many_lines)]
async fn main() {
    let cli = Cli::parse();

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
    );

    let mut journal_client = if needs_journal {
        match execute::connect(&config).await {
            Ok(channel) => Some(ConfigServiceClient::new(channel)),
            Err(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
    } else {
        None
    };

    let result = match cli.command {
        Commands::Status { node, .. } => {
            let node_id = node.unwrap_or_else(|| "local".to_string());
            execute::status(journal_client.as_mut().unwrap(), &node_id).await
        }
        Commands::Log { n, scope } => {
            execute::log(journal_client.as_mut().unwrap(), n, scope.as_deref()).await
        }
        Commands::Commit { m } => {
            let vcluster = config
                .default_vcluster
                .as_deref()
                .unwrap_or("default")
                .to_string();
            // TODO: resolve principal/role from OIDC token
            execute::commit(
                journal_client.as_mut().unwrap(),
                &m,
                &vcluster,
                "cli-user",
                "pact-platform-admin",
            )
            .await
        }
        Commands::Rollback { seq } => {
            let vcluster = config
                .default_vcluster
                .as_deref()
                .unwrap_or("default")
                .to_string();
            execute::rollback(
                journal_client.as_mut().unwrap(),
                seq,
                &vcluster,
                "cli-user",
                "pact-platform-admin",
            )
            .await
        }
        Commands::Diff { node, committed: _ } => {
            // Diff queries entries, not a dedicated RPC
            let scope_filter = node.as_deref().map(|n| format!("node:{n}"));
            execute::log(journal_client.as_mut().unwrap(), 50, scope_filter.as_deref()).await
        }

        // Commands that need agent gRPC
        Commands::Exec { node: _, command } => {
            match pact_cli::commands::exec::parse_exec_command(&command) {
                Ok((cmd, args)) => {
                    // TODO: resolve agent address from node_id (for now, assume localhost:9445)
                    let agent_addr = "http://127.0.0.1:9445";
                    match execute::connect_agent(agent_addr).await {
                        Ok(channel) => {
                            let token = config
                                .resolve_token()
                                .unwrap_or_else(|_| "dev-token".to_string());
                            execute::exec_remote(channel, &token, &cmd, &args).await
                        }
                        Err(e) => Err(e),
                    }
                }
                Err(e) => Err(anyhow::anyhow!("{e}")),
            }
        }
        Commands::Shell { node } => {
            Ok(format!("shell on {node} (agent gRPC not yet wired)"))
        }
        Commands::Emergency { action } => match action {
            EmergencySubcommand::Start { reason } => {
                Ok(format!("emergency start: {reason} (not yet wired)"))
            }
            EmergencySubcommand::End { force } => {
                Ok(format!("emergency end (force={force}) (not yet wired)"))
            }
        },
        Commands::Approve { action } => match action {
            ApproveSubcommand::List => Ok("approve list (not yet wired)".to_string()),
            ApproveSubcommand::Accept { id } => {
                Ok(format!("approve accept {id} (not yet wired)"))
            }
            ApproveSubcommand::Deny { id, m } => {
                Ok(format!("approve deny {id}: {m} (not yet wired)"))
            }
        },
        Commands::Service { action } => match action {
            ServiceSubcommand::Status { name } => {
                Ok(format!("service status {name:?} (not yet wired)"))
            }
            ServiceSubcommand::Restart { name } => {
                Ok(format!("service restart {name} (not yet wired)"))
            }
            ServiceSubcommand::Logs { name } => {
                Ok(format!("service logs {name} (not yet wired)"))
            }
        },
        Commands::Cap { node } => Ok(format!("cap {node:?} (not yet wired)")),
        Commands::Watch { vcluster } => {
            Ok(format!("watch {vcluster:?} (not yet wired)"))
        }
        Commands::Apply { spec } => Ok(format!("apply {spec} (not yet wired)")),
        Commands::Extend { mins } => {
            Ok(format!("extend {mins} min (not yet wired)"))
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
