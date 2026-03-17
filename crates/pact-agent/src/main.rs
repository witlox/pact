//! pact-agent — per-node init system, process supervisor, and shell server.
//!
//! See docs/architecture/agent-design.md for design documentation.

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use tracing::{error, info};

use pact_agent::boot;
use pact_agent::shell::auth::AuthConfig;
use pact_agent::shell::exec::ExecConfig;
use pact_agent::shell::grpc_service::ShellServiceImpl;
use pact_agent::shell::ShellServer;
use pact_common::config::PactConfig;
use pact_common::proto::shell::shell_service_server::ShellServiceServer;

/// pact-agent: config management agent for HPC/AI nodes.
#[derive(Parser, Debug)]
#[command(name = "pact-agent", about = "Config management agent for HPC/AI nodes")]
struct Args {
    /// Path to configuration file.
    #[arg(long, default_value = "/etc/pact/agent.toml", env = "PACT_AGENT_CONFIG")]
    config: PathBuf,

    /// Override node ID (takes precedence over config file).
    #[arg(long, env = "PACT_AGENT_NODE_ID")]
    node_id: Option<String>,

    /// Override vCluster (takes precedence over config file).
    #[arg(long, env = "PACT_AGENT_VCLUSTER")]
    vcluster: Option<String>,
}

#[tokio::main]
#[allow(clippy::too_many_lines)]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    info!(config = %args.config.display(), "Starting pact-agent");

    // Load configuration
    let config_str = std::fs::read_to_string(&args.config).unwrap_or_else(|e| {
        error!(path = %args.config.display(), "Failed to read config: {e}");
        std::process::exit(1);
    });
    let pact_config: PactConfig = toml::from_str(&config_str)?;
    let mut agent_config =
        pact_config.agent.ok_or_else(|| anyhow::anyhow!("missing [agent] section in config"))?;

    // Apply CLI overrides
    if let Some(node_id) = args.node_id {
        agent_config.node_id = node_id;
    }
    if let Some(vcluster) = args.vcluster {
        agent_config.vcluster = Some(vcluster);
    }

    info!(
        node_id = %agent_config.node_id,
        vcluster = agent_config.vcluster.as_deref().unwrap_or("(none)"),
        enforcement_mode = %agent_config.enforcement_mode,
        "Agent configured"
    );

    // Connect to journal (optional — agent can start without it)
    let journal_client = pact_agent::journal_client::try_connect(&agent_config.journal).await;

    // Execute boot sequence — initializes all subsystems
    let boot_result = boot::boot(&agent_config, journal_client.as_ref()).await?;

    // Start config subscription for live updates from journal
    let (subscription, mut action_rx) = boot::start_subscription(&agent_config);

    // Spawn subscription streaming loop (if connected to journal)
    if let Some(ref client) = journal_client {
        let sub = subscription.clone();
        let boot_client = client.boot_config();
        tokio::spawn(async move {
            sub.run(boot_client).await;
        });
    }

    // Spawn config update handler
    let handles = boot::ConfigActionHandles {
        drift_evaluator: boot_result.drift_evaluator.clone(),
        commit_window: boot_result.commit_window.clone(),
        cached_policy: boot_result.cached_policy.clone(),
        blacklist: agent_config.blacklist.clone(),
    };
    tokio::spawn(async move {
        while let Some(action) = action_rx.recv().await {
            boot::handle_config_action(action, &handles).await;
        }
    });

    // Start shell gRPC server
    let shell_listen = agent_config.shell.listen.clone();
    let auth_config = if let Some(ref auth) = agent_config.shell.auth {
        AuthConfig {
            issuer: auth.issuer.clone(),
            audience: auth.audience.clone(),
            hmac_secret: auth.hmac_secret.as_ref().map(|s| s.as_bytes().to_vec()),
            jwks_url: auth.jwks_url.clone(),
        }
    } else {
        info!("No [agent.shell.auth] configured — shell auth will require JWKS (fail-closed)");
        AuthConfig {
            issuer: String::new(),
            audience: String::new(),
            hmac_secret: None,
            jwks_url: None,
        }
    };
    let mut shell_server = ShellServer::new(
        auth_config,
        ExecConfig::default(),
        agent_config.node_id.clone(),
        agent_config.vcluster.clone().unwrap_or_default(),
        agent_config.shell.whitelist_mode == "learning",
        10, // max concurrent sessions
    );
    if let Some(ref client) = journal_client {
        shell_server = shell_server.with_journal_client(client.clone());
    }
    shell_server = shell_server.with_commit_window(boot_result.commit_window.clone());
    let shell_server = Arc::new(shell_server);
    let shell_svc = ShellServiceImpl::new(shell_server, boot_result.commit_window.clone());

    let shell_listener = tokio::net::TcpListener::bind(&shell_listen).await?;
    let shell_addr = shell_listener.local_addr()?;
    info!(%shell_addr, "Shell gRPC server listening");
    tokio::spawn(async move {
        let incoming = tokio_stream::wrappers::TcpListenerStream::new(shell_listener);
        if let Err(e) = tonic::transport::Server::builder()
            .add_service(ShellServiceServer::new(shell_svc))
            .serve_with_incoming(incoming)
            .await
        {
            error!(error = %e, "Shell gRPC server error");
        }
    });

    info!(
        config_state = ?boot_result.config_state,
        "Agent ready — steady state"
    );

    // Run until interrupted
    tokio::signal::ctrl_c().await?;

    // Graceful shutdown — stop all supervised services
    info!("Shutting down — stopping supervised services");
    if let Err(e) = boot_result.supervisor.stop_all(&[]).await {
        error!(error = %e, "Error stopping services during shutdown");
    }

    info!("Shutdown complete");
    Ok(())
}
