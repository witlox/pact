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
            | Commands::Emergency { .. }
            | Commands::Approve { .. }
            | Commands::Watch { .. }
            | Commands::Apply { .. }
            | Commands::Exec { .. }
            | Commands::Service { .. }
            | Commands::Cap { .. }
            | Commands::Extend { .. }
    );

    let journal_channel = if needs_journal {
        match execute::connect(&config).await {
            Ok(channel) => Some(channel),
            Err(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
    } else {
        None
    };
    let mut journal_client =
        journal_channel.as_ref().map(|ch| ConfigServiceClient::new(ch.clone()));

    // Resolve identity from token (if available)
    let token = config.resolve_token().unwrap_or_default();
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
                    match execute::resolve_agent_address(&node, journal_channel.as_ref().unwrap())
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
            let agent_addr = match execute::resolve_agent_address(
                &node,
                journal_channel.as_ref().unwrap(),
            )
            .await
            {
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
            let channel = journal_channel.as_ref().unwrap();
            match action {
                ApproveSubcommand::List => execute::approve_list(channel, None).await,
                ApproveSubcommand::Accept { id } => {
                    execute::approve_decide(
                        channel,
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
                        channel,
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
                match execute::resolve_agent_address("local", journal_channel.as_ref().unwrap())
                    .await
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
                match execute::resolve_agent_address(node_id, journal_channel.as_ref().unwrap())
                    .await
                {
                    Ok(addr) => addr,
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                };
            match execute::connect_agent(&agent_addr).await {
                Ok(channel) => execute::list_agent_commands(channel).await,
                Err(e) => Err(e),
            }
        }
        Commands::Watch { vcluster } => {
            let vc =
                vcluster.as_deref().or(config.default_vcluster.as_deref()).unwrap_or("default");
            execute::watch(journal_channel.as_ref().unwrap(), vc).await
        }
        Commands::Apply { spec } => {
            execute::apply(journal_client.as_mut().unwrap(), &spec, &principal, &role).await
        }
        Commands::Extend { mins } => {
            let agent_addr =
                match execute::resolve_agent_address("local", journal_channel.as_ref().unwrap())
                    .await
                {
                    Ok(addr) => addr,
                    Err(e) => {
                        eprintln!("Error: {e}");
                        std::process::exit(1);
                    }
                };
            match execute::connect_agent(&agent_addr).await {
                Ok(channel) => execute::extend(channel, mins).await,
                Err(e) => Err(e),
            }
        }
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
