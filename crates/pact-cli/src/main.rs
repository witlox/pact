//! pact CLI — configuration management and admin operations for HPC/AI.
//!
//! See `docs/architecture/cli-design.md` for command reference.

use clap::{Parser, Subcommand};
use pact_cli::commands::CliConfig;

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

fn main() {
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

    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::WARN.into()),
        )
        .init();

    // Dispatch to command handlers.
    // Each handler will create a gRPC client and execute the request.
    // For now, print the parsed command for verification.
    match cli.command {
        Commands::Status { node, vcluster } => {
            let vc = vcluster.as_deref().or(config.default_vcluster.as_deref());
            eprintln!(
                "pact status: node={:?}, vcluster={:?}, endpoint={}",
                node, vc, config.endpoint,
            );
            eprintln!("(gRPC client not yet connected — journal endpoint needed)");
        }
        Commands::Diff { node, committed } => {
            eprintln!("pact diff: node={node:?}, committed={committed}");
        }
        Commands::Log { n, scope } => {
            eprintln!("pact log: n={n}, scope={scope:?}");
        }
        Commands::Commit { m } => {
            eprintln!("pact commit: message={m:?}");
        }
        Commands::Rollback { seq } => {
            eprintln!("pact rollback: target_seq={seq}");
        }
        Commands::Exec { node, command } => {
            match pact_cli::commands::exec::parse_exec_command(&command) {
                Ok((cmd, args)) => {
                    eprintln!("pact exec {node}: {cmd} {args:?}");
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        }
        Commands::Shell { node } => {
            eprintln!("pact shell: node={node}");
        }
        Commands::Emergency { action } => match action {
            EmergencySubcommand::Start { reason } => {
                eprintln!("pact emergency start: reason={reason:?}");
            }
            EmergencySubcommand::End { force } => {
                eprintln!("pact emergency end: force={force}");
            }
        },
        Commands::Approve { action } => match action {
            ApproveSubcommand::List => {
                eprintln!("pact approve list");
            }
            ApproveSubcommand::Accept { id } => {
                eprintln!("pact approve accept: id={id}");
            }
            ApproveSubcommand::Deny { id, m } => {
                eprintln!("pact approve deny: id={id}, reason={m:?}");
            }
        },
        Commands::Service { action } => match action {
            ServiceSubcommand::Status { name } => {
                eprintln!("pact service status: name={name:?}");
            }
            ServiceSubcommand::Restart { name } => {
                eprintln!("pact service restart: name={name}");
            }
            ServiceSubcommand::Logs { name } => {
                eprintln!("pact service logs: name={name}");
            }
        },
        Commands::Cap { node } => {
            eprintln!("pact cap: node={node:?}");
        }
        Commands::Watch { vcluster } => {
            eprintln!("pact watch: vcluster={vcluster:?}");
        }
        Commands::Apply { spec } => {
            eprintln!("pact apply: spec={spec}");
        }
        Commands::Extend { mins } => {
            eprintln!("pact extend: mins={mins}");
        }
    }
}
