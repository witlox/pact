//! pact-journal — distributed immutable configuration log (Raft quorum).
//!
//! See docs/architecture/journal-design.md for design documentation.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use openraft::impls::BasicNode;
use openraft::Raft;
use raft_hpc_core::{FileLogStore, GrpcNetworkFactory, HpcStateMachine, RaftTransportServer};
use tokio::sync::RwLock;
use tracing::{error, info};

use pact_common::proto::journal::config_service_server::ConfigServiceServer;
use pact_common::proto::policy::policy_service_server::PolicyServiceServer;
use pact_common::proto::stream::boot_config_service_server::BootConfigServiceServer;
use pact_journal::auth::auth_interceptor;
use pact_journal::boot_service::{BootConfigServiceImpl, ConfigUpdateNotifier};
use pact_journal::policy_service::PolicyServiceImpl;
use pact_journal::service::ConfigServiceImpl;
use pact_journal::telemetry::{telemetry_router, JournalMetrics, TelemetryState};
use pact_journal::{JournalState, JournalTypeConfig};

/// pact-journal: distributed immutable configuration log.
#[derive(Parser, Debug)]
#[command(name = "pact-journal", about = "Immutable config log for HPC/AI clusters")]
struct Args {
    /// This node's Raft ID (1-based).
    #[arg(long, env = "PACT_JOURNAL_NODE_ID")]
    node_id: u64,

    /// Listen address for Raft + gRPC (e.g. "0.0.0.0:9443").
    #[arg(long, default_value = "0.0.0.0:9443", env = "PACT_JOURNAL_LISTEN")]
    listen: String,

    /// Raft peer addresses in "id=addr" format (e.g. "1=host1:9443,2=host2:9443,3=host3:9443").
    #[arg(long, value_delimiter = ',', env = "PACT_JOURNAL_PEERS")]
    peers: Vec<String>,

    /// Data directory for WAL, snapshots, and vote state.
    #[arg(long, default_value = "/var/lib/pact/journal", env = "PACT_JOURNAL_DATA_DIR")]
    data_dir: PathBuf,

    /// Raft snapshot directory (defaults to `data_dir/snapshots`).
    #[arg(long, env = "PACT_JOURNAL_SNAPSHOT_DIR")]
    snapshot_dir: Option<PathBuf>,

    /// Listen address for metrics/health HTTP (e.g. "0.0.0.0:9091").
    #[arg(long, default_value = "0.0.0.0:9091", env = "PACT_JOURNAL_METRICS_LISTEN")]
    metrics_listen: String,

    /// Bootstrap a new cluster (only on first start of node 1).
    #[arg(long, default_value_t = false)]
    bootstrap: bool,
}

fn parse_peers(peers: &[String]) -> BTreeMap<u64, BasicNode> {
    let mut members = BTreeMap::new();
    for peer in peers {
        let (id_str, addr) = peer
            .split_once('=')
            .unwrap_or_else(|| panic!("Invalid peer format '{peer}', expected 'id=addr'"));
        let id: u64 = id_str.parse().unwrap_or_else(|_| panic!("Invalid peer id '{id_str}'"));
        members.insert(id, BasicNode::new(addr.to_string()));
    }
    members
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
    info!(
        node_id = args.node_id,
        listen = %args.listen,
        data_dir = %args.data_dir.display(),
        "Starting pact-journal"
    );

    // Parse peer addresses
    let members = parse_peers(&args.peers);

    // Raft configuration
    let config = Arc::new(
        openraft::Config {
            heartbeat_interval: 500,
            election_timeout_min: 1500,
            election_timeout_max: 3000,
            snapshot_policy: openraft::SnapshotPolicy::LogsSinceLast(10000),
            ..Default::default()
        }
        .validate()?,
    );

    // Data directories
    std::fs::create_dir_all(&args.data_dir)?;
    let snapshot_dir = args.snapshot_dir.unwrap_or_else(|| args.data_dir.join("snapshots"));

    // Create Raft components using raft-hpc-core
    let state = Arc::new(RwLock::new(JournalState::default()));
    let log_store = FileLogStore::<JournalTypeConfig>::new(&args.data_dir)?;
    let sm = HpcStateMachine::with_snapshot_dir(Arc::clone(&state), snapshot_dir)?;
    let network = GrpcNetworkFactory::new();

    // Register peer addresses in the network factory
    for (id, node) in &members {
        network.register(*id, node.addr.clone()).await;
    }

    // Create Raft instance
    let raft: Raft<JournalTypeConfig> =
        Raft::new(args.node_id, config, network, log_store, sm).await?;

    // Bootstrap cluster if requested
    if args.bootstrap {
        info!("Bootstrapping new cluster with {} members", members.len());
        raft.initialize(members).await?;
    }

    // Start telemetry HTTP server (metrics + health)
    let telemetry_state = TelemetryState {
        raft: raft.clone(),
        journal: Arc::clone(&state),
        metrics: JournalMetrics::default(),
    };
    let metrics_listener = tokio::net::TcpListener::bind(&args.metrics_listen).await?;
    let metrics_addr = metrics_listener.local_addr()?;
    info!(%metrics_addr, "Telemetry server listening");
    tokio::spawn(async move {
        axum::serve(metrics_listener, telemetry_router(telemetry_state))
            .await
            .inspect_err(|e| error!("Telemetry server error: {e}"))
            .ok();
    });

    // Shared notifier for live config push to subscribers
    let notifier = ConfigUpdateNotifier::default();

    // Start gRPC transport server with application services
    let raft_server = RaftTransportServer::new(raft.clone());
    let config_service = ConfigServiceImpl::new(raft.clone(), Arc::clone(&state), notifier.clone());
    let policy_service = PolicyServiceImpl::new(raft.clone(), Arc::clone(&state));
    let boot_service = BootConfigServiceImpl::new(Arc::clone(&state), notifier);
    let listener = tokio::net::TcpListener::bind(&args.listen).await?;
    let addr = listener.local_addr()?;
    info!(%addr, "gRPC server listening");

    let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
    tonic::transport::Server::builder()
        .add_service(raft_hpc_core::proto::raft_service_server::RaftServiceServer::new(raft_server))
        .add_service(ConfigServiceServer::with_interceptor(config_service, auth_interceptor))
        .add_service(PolicyServiceServer::with_interceptor(policy_service, auth_interceptor))
        .add_service(BootConfigServiceServer::with_interceptor(boot_service, auth_interceptor))
        .serve_with_incoming(incoming)
        .await
        .inspect_err(|e| error!("gRPC server error: {e}"))?;

    Ok(())
}
