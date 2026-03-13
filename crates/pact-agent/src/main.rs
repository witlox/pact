//! pact-agent — per-node init system, process supervisor, and shell server.
//!
//! See docs/architecture/agent-design.md for design documentation.

use std::path::PathBuf;

use clap::Parser;
use tracing::{error, info};

use pact_agent::boot;
use pact_common::config::PactConfig;

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
        agent_config.vcluster = vcluster;
    }

    info!(
        node_id = %agent_config.node_id,
        vcluster = %agent_config.vcluster,
        enforcement_mode = %agent_config.enforcement_mode,
        "Agent configured"
    );

    // Execute boot sequence — initializes all subsystems
    let boot_result = boot::boot(&agent_config).await?;

    // Start config subscription for live updates from journal
    let (_subscription, mut action_rx) = boot::start_subscription(&agent_config);

    // Spawn config update handler
    let drift_evaluator = boot_result.drift_evaluator.clone();
    let blacklist = agent_config.blacklist.clone();
    tokio::spawn(async move {
        while let Some(action) = action_rx.recv().await {
            boot::handle_config_action(action, &drift_evaluator, &blacklist).await;
        }
    });

    // TODO Phase 3: Start shell server
    info!(
        config_state = ?boot_result.config_state,
        "Agent ready — steady state"
    );

    // Run until interrupted
    tokio::signal::ctrl_c().await?;

    info!("Shutting down");
    Ok(())
}
