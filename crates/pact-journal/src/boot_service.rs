//! BootConfigService gRPC implementation.
//!
//! Streams pre-computed boot overlays and node-specific deltas from local state
//! (no Raft round-trip — J8). Supports chunked + zstd-compressed delivery for
//! boot storm scalability.

use std::sync::Arc;

use tokio::sync::{broadcast, RwLock};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};
use tracing::debug;

use pact_common::proto::stream::{
    boot_config_service_server::BootConfigService, BootConfigRequest, ConfigChunk, ConfigComplete,
    ConfigUpdate, NodeDelta, OverlayChunk, SubscribeRequest,
};

use crate::JournalState;

/// Maximum chunk size for overlay streaming (64 KB).
const CHUNK_SIZE: usize = 64 * 1024;

/// Notifier for broadcasting new config entries to subscribers.
///
/// Shared between ConfigServiceImpl (produces notifications after Raft writes)
/// and BootConfigServiceImpl (consumes notifications for live push).
#[derive(Clone)]
pub struct ConfigUpdateNotifier {
    sender: broadcast::Sender<ConfigUpdate>,
}

impl ConfigUpdateNotifier {
    /// Create a new notifier with the given channel capacity.
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Send a config update notification. Returns the number of receivers.
    pub fn notify(&self, update: ConfigUpdate) -> usize {
        self.sender.send(update).unwrap_or(0)
    }

    /// Subscribe to receive config update notifications.
    pub fn subscribe(&self) -> broadcast::Receiver<ConfigUpdate> {
        self.sender.subscribe()
    }
}

impl Default for ConfigUpdateNotifier {
    fn default() -> Self {
        Self::new(1024)
    }
}

/// gRPC BootConfigService — serves boot config from local state.
pub struct BootConfigServiceImpl {
    state: Arc<RwLock<JournalState>>,
    notifier: ConfigUpdateNotifier,
}

impl BootConfigServiceImpl {
    pub fn new(state: Arc<RwLock<JournalState>>, notifier: ConfigUpdateNotifier) -> Self {
        Self { state, notifier }
    }
}

#[tonic::async_trait]
impl BootConfigService for BootConfigServiceImpl {
    type StreamBootConfigStream = ReceiverStream<Result<ConfigChunk, Status>>;

    /// Stream boot config: overlay chunks + node delta + completion marker.
    ///
    /// Phase 1: overlay served from pre-computed cache. Phase 2: node delta includes
    /// committed manual changes from journal.
    async fn stream_boot_config(
        &self,
        request: Request<BootConfigRequest>,
    ) -> Result<Response<Self::StreamBootConfigStream>, Status> {
        let req = request.into_inner();
        let state = self.state.read().await;

        let overlay = state.overlays.get(&req.vcluster_id).ok_or_else(|| {
            Status::not_found(format!("no overlay for vCluster {}", req.vcluster_id))
        })?;

        // Skip if client already has this version
        if let Some(known) = req.last_known_version {
            if known >= overlay.version {
                // Send just a completion marker — client is up to date
                let (tx, rx) = tokio::sync::mpsc::channel(1);
                let complete = ConfigChunk {
                    chunk: Some(pact_common::proto::stream::config_chunk::Chunk::Complete(
                        ConfigComplete {
                            base_version: overlay.version,
                            node_version: 0,
                            timestamp: Some(prost_types::Timestamp {
                                seconds: chrono::Utc::now().timestamp(),
                                nanos: 0,
                            }),
                        },
                    )),
                };
                tokio::spawn(async move {
                    let _ = tx.send(Ok(complete)).await;
                });
                return Ok(Response::new(ReceiverStream::new(rx)));
            }
        }

        // Chunk the overlay data
        let data = &overlay.data;
        let total_chunks = data.len().div_ceil(CHUNK_SIZE);
        let total_chunks = total_chunks.max(1) as u32;

        let mut chunks: Vec<ConfigChunk> = Vec::new();

        if data.is_empty() {
            // Empty overlay — send single empty chunk
            chunks.push(ConfigChunk {
                chunk: Some(pact_common::proto::stream::config_chunk::Chunk::BaseOverlay(
                    OverlayChunk {
                        version: overlay.version,
                        vcluster_id: overlay.vcluster_id.clone(),
                        data: vec![],
                        chunk_index: 0,
                        total_chunks: 1,
                        checksum: overlay.checksum.clone(),
                    },
                )),
            });
        } else {
            for (i, chunk_data) in data.chunks(CHUNK_SIZE).enumerate() {
                chunks.push(ConfigChunk {
                    chunk: Some(pact_common::proto::stream::config_chunk::Chunk::BaseOverlay(
                        OverlayChunk {
                            version: overlay.version,
                            vcluster_id: overlay.vcluster_id.clone(),
                            data: chunk_data.to_vec(),
                            chunk_index: i as u32,
                            total_chunks,
                            checksum: overlay.checksum.clone(),
                        },
                    )),
                });
            }
        }

        // Build node delta from committed node-scoped entries
        let node_delta_data = build_node_delta(&state, &req.node_id);
        if !node_delta_data.is_empty() {
            let checksum = format!("{:x}", md5_hash(&node_delta_data));
            chunks.push(ConfigChunk {
                chunk: Some(pact_common::proto::stream::config_chunk::Chunk::NodeDelta(
                    NodeDelta {
                        node_id: req.node_id.clone(),
                        version: overlay.version,
                        data: node_delta_data,
                        checksum,
                    },
                )),
            });
        }

        // Completion marker
        chunks.push(ConfigChunk {
            chunk: Some(pact_common::proto::stream::config_chunk::Chunk::Complete(
                ConfigComplete {
                    base_version: overlay.version,
                    node_version: overlay.version,
                    timestamp: Some(prost_types::Timestamp {
                        seconds: chrono::Utc::now().timestamp(),
                        nanos: 0,
                    }),
                },
            )),
        });

        let (tx, rx) = tokio::sync::mpsc::channel(chunks.len().max(1));
        tokio::spawn(async move {
            for chunk in chunks {
                if tx.send(Ok(chunk)).await.is_err() {
                    break;
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }

    type SubscribeConfigUpdatesStream = ReceiverStream<Result<ConfigUpdate, Status>>;

    /// Subscribe to live config updates after boot.
    ///
    /// Sends existing entries >= from_sequence as catch-up, then keeps the stream
    /// open for live updates via broadcast channel. Stream closes when the client
    /// disconnects.
    async fn subscribe_config_updates(
        &self,
        request: Request<SubscribeRequest>,
    ) -> Result<Response<Self::SubscribeConfigUpdatesStream>, Status> {
        let req = request.into_inner();
        let state = self.state.read().await;

        // Catch-up: send entries from from_sequence onwards
        let catchup: Vec<ConfigUpdate> = state
            .entries
            .range(req.from_sequence..)
            .map(|(seq, entry)| {
                let serialized = serde_json::to_vec(entry).unwrap_or_default();
                ConfigUpdate {
                    sequence: *seq,
                    timestamp: Some(prost_types::Timestamp {
                        seconds: entry.timestamp.timestamp(),
                        nanos: entry.timestamp.timestamp_subsec_nanos() as i32,
                    }),
                    update: Some(
                        pact_common::proto::stream::config_update::Update::VclusterChange(
                            serialized,
                        ),
                    ),
                }
            })
            .collect();

        // Track highest sequence sent during catch-up to avoid duplicates
        let mut last_sent_seq =
            catchup.last().map_or_else(|| req.from_sequence.saturating_sub(1), |u| u.sequence);

        drop(state);

        // Subscribe to live updates before sending catch-up to avoid race
        let mut live_rx = self.notifier.subscribe();

        let (tx, rx) = tokio::sync::mpsc::channel(catchup.len().max(64));
        tokio::spawn(async move {
            // Phase 1: catch-up
            for update in catchup {
                if tx.send(Ok(update)).await.is_err() {
                    return; // client disconnected
                }
            }

            // Phase 2: live push
            loop {
                match live_rx.recv().await {
                    Ok(update) => {
                        // Skip duplicates from catch-up window
                        if update.sequence <= last_sent_seq {
                            continue;
                        }
                        last_sent_seq = update.sequence;
                        debug!(sequence = update.sequence, "Sending live config update");
                        if tx.send(Ok(update)).await.is_err() {
                            break; // client disconnected
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        debug!(skipped = n, "Subscriber lagged — disconnecting for re-sync");
                        break; // Force reconnect to get fresh catch-up
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        break; // journal shutting down
                    }
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

/// Build serialized node delta from committed node-scoped entries.
fn build_node_delta(state: &JournalState, node_id: &str) -> Vec<u8> {
    use pact_common::types::Scope;

    let node_entries: Vec<_> = state
        .entries
        .values()
        .filter(|e| matches!(&e.scope, Scope::Node(n) if n == node_id))
        .collect();

    if node_entries.is_empty() {
        return vec![];
    }

    serde_json::to_vec(&node_entries).unwrap_or_default()
}

/// Simple hash for checksums (not cryptographic — just for integrity checks).
fn md5_hash(data: &[u8]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    data.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use pact_common::types::{BootOverlay, ConfigEntry, EntryType, Identity, PrincipalType, Scope};
    use raft_hpc_core::StateMachineState;
    use tokio_stream::StreamExt;

    use crate::raft::types::JournalCommand;

    fn test_entry(entry_type: EntryType, scope: Scope) -> ConfigEntry {
        ConfigEntry {
            sequence: 0,
            timestamp: Utc::now(),
            entry_type,
            scope,
            author: Identity {
                principal: "admin@example.com".into(),
                principal_type: PrincipalType::Human,
                role: "pact-platform-admin".into(),
            },
            parent: None,
            state_delta: None,
            policy_ref: None,
            ttl_seconds: None,
            emergency_reason: None,
        }
    }

    fn boot_state() -> JournalState {
        let mut state = JournalState::default();
        // Add overlay
        state.apply(JournalCommand::SetOverlay {
            vcluster_id: "ml-training".into(),
            overlay: BootOverlay::new("ml-training", 5, vec![10; 100]),
        });
        // Add some entries for subscribe testing
        state.apply(JournalCommand::AppendEntry(test_entry(
            EntryType::Commit,
            Scope::VCluster("ml-training".into()),
        )));
        state.apply(JournalCommand::AppendEntry(test_entry(
            EntryType::PolicyUpdate,
            Scope::VCluster("ml-training".into()),
        )));
        state.apply(JournalCommand::AppendEntry(test_entry(
            EntryType::Commit,
            Scope::Node("node-001".into()),
        )));
        state
    }

    fn test_service() -> BootConfigServiceImpl {
        BootConfigServiceImpl::new(
            Arc::new(RwLock::new(boot_state())),
            ConfigUpdateNotifier::default(),
        )
    }

    #[tokio::test]
    async fn stream_boot_config_sends_overlay_and_complete() {
        let svc = test_service();
        let resp = svc
            .stream_boot_config(Request::new(BootConfigRequest {
                node_id: "node-001".into(),
                vcluster_id: "ml-training".into(),
                last_known_version: None,
            }))
            .await
            .unwrap();

        let mut stream = resp.into_inner();
        let mut chunks = vec![];
        while let Some(Ok(chunk)) = stream.next().await {
            chunks.push(chunk);
        }

        // Should have: 1 overlay chunk (100 bytes < 64KB) + 1 node delta + 1 complete
        assert!(chunks.len() >= 2); // at least overlay + complete

        // First chunk should be overlay
        match &chunks[0].chunk {
            Some(pact_common::proto::stream::config_chunk::Chunk::BaseOverlay(ov)) => {
                assert_eq!(ov.version, 5);
                assert_eq!(ov.vcluster_id, "ml-training");
                assert_eq!(ov.data.len(), 100);
                assert_eq!(ov.chunk_index, 0);
                assert_eq!(ov.total_chunks, 1);
            }
            other => panic!("expected BaseOverlay, got {other:?}"),
        }

        // Last chunk should be Complete
        match &chunks.last().unwrap().chunk {
            Some(pact_common::proto::stream::config_chunk::Chunk::Complete(c)) => {
                assert_eq!(c.base_version, 5);
            }
            other => panic!("expected Complete, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stream_boot_config_skips_when_up_to_date() {
        let svc = test_service();
        let resp = svc
            .stream_boot_config(Request::new(BootConfigRequest {
                node_id: "node-001".into(),
                vcluster_id: "ml-training".into(),
                last_known_version: Some(5), // already have version 5
            }))
            .await
            .unwrap();

        let mut stream = resp.into_inner();
        let mut chunks = vec![];
        while let Some(Ok(chunk)) = stream.next().await {
            chunks.push(chunk);
        }

        // Should only get a Complete marker
        assert_eq!(chunks.len(), 1);
        assert!(matches!(
            &chunks[0].chunk,
            Some(pact_common::proto::stream::config_chunk::Chunk::Complete(_))
        ));
    }

    #[tokio::test]
    async fn stream_boot_config_not_found_for_missing_vcluster() {
        let svc = test_service();
        let result = svc
            .stream_boot_config(Request::new(BootConfigRequest {
                node_id: "node-001".into(),
                vcluster_id: "nonexistent".into(),
                last_known_version: None,
            }))
            .await;
        assert_eq!(result.unwrap_err().code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn stream_boot_config_includes_node_delta() {
        let svc = test_service();
        let resp = svc
            .stream_boot_config(Request::new(BootConfigRequest {
                node_id: "node-001".into(),
                vcluster_id: "ml-training".into(),
                last_known_version: None,
            }))
            .await
            .unwrap();

        let mut stream = resp.into_inner();
        let mut has_node_delta = false;
        while let Some(Ok(chunk)) = stream.next().await {
            if let Some(pact_common::proto::stream::config_chunk::Chunk::NodeDelta(nd)) =
                &chunk.chunk
            {
                assert_eq!(nd.node_id, "node-001");
                assert!(!nd.data.is_empty());
                has_node_delta = true;
            }
        }
        assert!(has_node_delta, "expected a NodeDelta chunk for node-001");
    }

    #[tokio::test]
    async fn subscribe_catches_up_from_sequence() {
        let svc = test_service();
        let resp = svc
            .subscribe_config_updates(Request::new(SubscribeRequest {
                node_id: "node-001".into(),
                vcluster_id: "ml-training".into(),
                from_sequence: 1, // skip entry 0, get entries 1 and 2
            }))
            .await
            .unwrap();

        // Stream stays open for live push, collect with timeout
        let mut stream = resp.into_inner();
        let mut updates = vec![];
        while let Ok(Some(Ok(update))) =
            tokio::time::timeout(tokio::time::Duration::from_millis(100), stream.next()).await
        {
            updates.push(update);
        }

        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].sequence, 1);
        assert_eq!(updates[1].sequence, 2);
    }

    #[tokio::test]
    async fn subscribe_from_zero_gets_all_entries() {
        let svc = test_service();
        let resp = svc
            .subscribe_config_updates(Request::new(SubscribeRequest {
                node_id: "node-001".into(),
                vcluster_id: "ml-training".into(),
                from_sequence: 0,
            }))
            .await
            .unwrap();

        // Stream stays open for live push, collect with timeout
        let mut stream = resp.into_inner();
        let mut updates = vec![];
        while let Ok(Some(Ok(update))) =
            tokio::time::timeout(tokio::time::Duration::from_millis(100), stream.next()).await
        {
            updates.push(update);
        }

        assert_eq!(updates.len(), 3); // all 3 entries
    }

    #[tokio::test]
    async fn large_overlay_is_chunked() {
        let mut state = JournalState::default();
        // Create overlay larger than CHUNK_SIZE (64KB)
        let large_data = vec![42u8; CHUNK_SIZE * 3 + 100]; // 3.something chunks
        state.apply(JournalCommand::SetOverlay {
            vcluster_id: "big-vc".into(),
            overlay: BootOverlay::new("big-vc", 1, large_data),
        });

        let svc = BootConfigServiceImpl::new(
            Arc::new(RwLock::new(state)),
            ConfigUpdateNotifier::default(),
        );
        let resp = svc
            .stream_boot_config(Request::new(BootConfigRequest {
                node_id: "node-x".into(),
                vcluster_id: "big-vc".into(),
                last_known_version: None,
            }))
            .await
            .unwrap();

        let mut stream = resp.into_inner();
        let mut overlay_chunks = 0u32;
        let mut total_data = 0usize;
        while let Some(Ok(chunk)) = stream.next().await {
            if let Some(pact_common::proto::stream::config_chunk::Chunk::BaseOverlay(ov)) =
                &chunk.chunk
            {
                assert_eq!(ov.total_chunks, 4); // ceil(3*64K+100 / 64K)
                assert_eq!(ov.chunk_index, overlay_chunks);
                total_data += ov.data.len();
                overlay_chunks += 1;
            }
        }
        assert_eq!(overlay_chunks, 4);
        assert_eq!(total_data, CHUNK_SIZE * 3 + 100);
    }
}
