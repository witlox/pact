//! In-process multi-node Raft cluster for integration testing.
//!
//! Bootstraps a 3-node pact-journal Raft cluster using in-process nodes with
//! separate data directories. Each node runs its own Raft instance, gRPC
//! services, and telemetry server.
//!
//! This is not a Docker container but uses the same lifecycle pattern —
//! create, wait for readiness, interact, drop to clean up.

use std::collections::BTreeMap;
use std::sync::Arc;

use openraft::async_runtime::WatchReceiver;

use openraft::impls::BasicNode;
use openraft::Raft;
use pact_common::proto::journal::config_service_server::ConfigServiceServer;
use pact_common::proto::policy::policy_service_server::PolicyServiceServer;
use pact_common::proto::stream::boot_config_service_server::BootConfigServiceServer;
use pact_journal::boot_service::{BootConfigServiceImpl, ConfigUpdateNotifier};
use pact_journal::policy_service::PolicyServiceImpl;
use pact_journal::service::ConfigServiceImpl;
use pact_journal::telemetry::{telemetry_router, JournalMetrics, TelemetryState};
use pact_journal::{JournalState, JournalTypeConfig};
use raft_hpc_core::{FileLogStore, GrpcNetworkFactory, HpcStateMachine};
use tokio::sync::RwLock;

/// A single Raft node with its gRPC and telemetry addresses.
pub struct RaftNode {
    pub node_id: u64,
    pub grpc_addr: String,
    pub metrics_addr: String,
    pub raft: Raft<JournalTypeConfig>,
    pub state: Arc<RwLock<JournalState>>,
    pub config_svc: ConfigServiceImpl,
    pub policy_svc: PolicyServiceImpl,
    pub boot_svc: BootConfigServiceImpl,
    _temp_dir: tempfile::TempDir,
    /// gRPC server handle — public so tests can abort it to simulate node failure.
    pub grpc_handle: tokio::task::JoinHandle<()>,
    _metrics_handle: tokio::task::JoinHandle<()>,
}

/// A 3-node Raft cluster for e2e testing.
pub struct RaftCluster {
    pub nodes: Vec<RaftNode>,
}

impl RaftCluster {
    /// Bootstrap a 3-node Raft cluster.
    ///
    /// Each node gets its own tempdir, Raft instance, gRPC server, and
    /// telemetry HTTP server on ephemeral ports.
    pub async fn bootstrap(node_count: u64) -> anyhow::Result<Self> {
        assert!((1..=5).contains(&node_count), "cluster size must be 1-5");

        // Allocate ephemeral ports by binding and releasing
        let mut grpc_addrs = Vec::new();
        let mut metrics_addrs = Vec::new();
        for _ in 0..node_count {
            let grpc_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let metrics_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            grpc_addrs.push(grpc_listener.local_addr()?.to_string());
            metrics_addrs.push(metrics_listener.local_addr()?.to_string());
            // Drop listeners — ports will be reused below
            // (tiny race window, acceptable for tests)
            drop(grpc_listener);
            drop(metrics_listener);
        }

        // Build membership map
        let mut members = BTreeMap::new();
        for (i, addr) in grpc_addrs.iter().enumerate() {
            members.insert((i + 1) as u64, BasicNode::new(addr.clone()));
        }

        // Start each node
        let mut nodes = Vec::new();
        for i in 0..node_count {
            let node_id = i + 1;
            let temp_dir = tempfile::tempdir()?;

            let config = Arc::new(
                openraft::Config {
                    heartbeat_interval: 300,
                    election_timeout_min: 900,
                    election_timeout_max: 1800,
                    ..Default::default()
                }
                .validate()?,
            );

            let state = Arc::new(RwLock::new(JournalState::default()));
            let log_store = FileLogStore::<JournalTypeConfig>::new(temp_dir.path())?;
            let snapshot_dir = temp_dir.path().join("snapshots");
            std::fs::create_dir_all(&snapshot_dir)?;
            let sm = HpcStateMachine::with_snapshot_dir(Arc::clone(&state), snapshot_dir)?;
            let network = GrpcNetworkFactory::new();

            // Register all peers
            for (peer_id, peer_node) in &members {
                network.register(*peer_id, peer_node.addr.clone()).await;
            }

            let raft: Raft<JournalTypeConfig> =
                Raft::new(node_id, config, network, log_store, sm).await?;

            // Bootstrap from node 1 only
            if node_id == 1 {
                raft.initialize(members.clone()).await?;
            }

            let notifier = ConfigUpdateNotifier::default();
            let config_svc =
                ConfigServiceImpl::new(raft.clone(), Arc::clone(&state), notifier.clone());
            let policy_svc = PolicyServiceImpl::new(raft.clone(), Arc::clone(&state));
            let boot_svc = BootConfigServiceImpl::new(Arc::clone(&state), notifier.clone());

            // Start gRPC server
            let grpc_addr = grpc_addrs[i as usize].clone();
            let grpc_listener = tokio::net::TcpListener::bind(&grpc_addr).await?;
            let actual_grpc_addr = grpc_listener.local_addr()?.to_string();

            let raft_server = raft_hpc_core::RaftTransportServer::new(raft.clone());
            let cs = ConfigServiceImpl::new(raft.clone(), Arc::clone(&state), notifier.clone());
            let ps = PolicyServiceImpl::new(raft.clone(), Arc::clone(&state));
            let bs = BootConfigServiceImpl::new(Arc::clone(&state), notifier);

            let grpc_handle = tokio::spawn(async move {
                let incoming = tokio_stream::wrappers::TcpListenerStream::new(grpc_listener);
                tonic::transport::Server::builder()
                    .add_service(raft_hpc_core::proto::raft_service_server::RaftServiceServer::new(
                        raft_server,
                    ))
                    .add_service(ConfigServiceServer::new(cs))
                    .add_service(PolicyServiceServer::new(ps))
                    .add_service(BootConfigServiceServer::new(bs))
                    .serve_with_incoming(incoming)
                    .await
                    .ok();
            });

            // Start telemetry server
            let metrics_addr_str = metrics_addrs[i as usize].clone();
            let metrics_listener = tokio::net::TcpListener::bind(&metrics_addr_str).await?;
            let actual_metrics_addr = metrics_listener.local_addr()?.to_string();

            let telemetry_state = TelemetryState {
                raft: raft.clone(),
                journal: Arc::clone(&state),
                metrics: JournalMetrics::default(),
                idp_url: String::new(),
                client_id: "pact-cli-test".into(),
            };
            let metrics_handle = tokio::spawn(async move {
                axum::serve(metrics_listener, telemetry_router(telemetry_state)).await.ok();
            });

            nodes.push(RaftNode {
                node_id,
                grpc_addr: actual_grpc_addr,
                metrics_addr: actual_metrics_addr,
                raft,
                state,
                config_svc,
                policy_svc,
                boot_svc,
                _temp_dir: temp_dir,
                grpc_handle,
                _metrics_handle: metrics_handle,
            });
        }

        // Wait for leader election
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        Ok(Self { nodes })
    }

    /// Find the current leader node, if any.
    pub async fn leader(&self) -> Option<&RaftNode> {
        for node in &self.nodes {
            let receiver = node.raft.metrics();
            let metrics = receiver.borrow_watched();
            if metrics.current_leader == Some(node.node_id) {
                return Some(node);
            }
        }
        None
    }

    /// Get a node by ID (1-based).
    pub fn node(&self, id: u64) -> &RaftNode {
        &self.nodes[(id - 1) as usize]
    }

    /// Get the gRPC address of the leader, for client connections.
    pub async fn leader_grpc_addr(&self) -> Option<String> {
        self.leader().await.map(|n| n.grpc_addr.clone())
    }
}
