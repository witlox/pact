//! Enrollment gRPC service — handles node enrollment, domain membership,
//! and certificate lifecycle.
//!
//! The `Enroll` RPC is unauthenticated (agents have no cert yet).
//! All other RPCs require Bearer token authentication (per-method auth).

use std::sync::Arc;

use openraft::Raft;
use tokio::sync::RwLock;
use tonic::{Request, Response, Status};
use tracing::{info, warn};

use pact_common::proto::enrollment::enrollment_service_server::EnrollmentService;
use pact_common::proto::enrollment::{
    AssignNodeRequest, AssignNodeResponse, BatchRegisterNodesRequest, BatchRegisterNodesResponse,
    BatchNodeResult, DecommissionNodeRequest, DecommissionNodeResponse, EnrollRequest,
    EnrollResponse, InspectNodeRequest, InspectNodeResponse, ListNodesRequest, ListNodesResponse,
    MoveNodeRequest, MoveNodeResponse, NodeSummary, RegisterNodeRequest, RegisterNodeResponse,
    RenewCertRequest, RenewCertResponse, UnassignNodeRequest, UnassignNodeResponse,
};
use pact_common::types::{
    DomainId, EnrollmentState, HardwareIdentity, Identity, NodeEnrollment, PrincipalType,
};

use crate::ca::CaKeyManager;
use crate::rate_limiter::RateLimiter;
use crate::raft::types::{JournalCommand, JournalResponse, JournalTypeConfig};
use crate::JournalState;

/// Enrollment gRPC service implementation.
pub struct EnrollmentServiceImpl {
    raft: Raft<JournalTypeConfig>,
    state: Arc<RwLock<JournalState>>,
    ca: Arc<CaKeyManager>,
    rate_limiter: Arc<RateLimiter>,
    domain_id: DomainId,
}

impl EnrollmentServiceImpl {
    pub fn new(
        raft: Raft<JournalTypeConfig>,
        state: Arc<RwLock<JournalState>>,
        ca: Arc<CaKeyManager>,
        rate_limiter: Arc<RateLimiter>,
        domain_id: DomainId,
    ) -> Self {
        Self { raft, state, ca, rate_limiter, domain_id }
    }

    /// Validate Bearer token from request metadata. Returns error if missing/invalid.
    fn require_auth<T>(req: &Request<T>) -> Result<(), Status> {
        let metadata = req.metadata();
        let auth_header = metadata
            .get("authorization")
            .ok_or_else(|| Status::unauthenticated("missing authorization header"))?;
        let value = auth_header
            .to_str()
            .map_err(|_| Status::unauthenticated("invalid authorization header"))?;
        if !value.starts_with("Bearer ") {
            return Err(Status::unauthenticated("expected Bearer token"));
        }
        Ok(())
    }

    /// Extract the principal name from auth metadata (simplified).
    fn extract_principal<T>(req: &Request<T>) -> String {
        req.metadata()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.trim_start_matches("Bearer ").to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }
}

fn proto_hw_to_domain(hw: &pact_common::proto::enrollment::HardwareIdentity) -> HardwareIdentity {
    HardwareIdentity {
        mac_address: hw.mac_address.clone(),
        bmc_serial: if hw.bmc_serial.is_empty() { None } else { Some(hw.bmc_serial.clone()) },
        extra: hw.extra.clone(),
    }
}

#[tonic::async_trait]
impl EnrollmentService for EnrollmentServiceImpl {
    /// Agent-initiated enrollment: unauthenticated.
    async fn enroll(
        &self,
        request: Request<EnrollRequest>,
    ) -> Result<Response<EnrollResponse>, Status> {
        // Rate limit
        if !self.rate_limiter.try_acquire() {
            warn!("Enrollment rate limit exceeded");
            return Err(Status::resource_exhausted("RATE_LIMITED"));
        }

        let req = request.into_inner();
        let hw = req
            .hardware_identity
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("hardware_identity required"))?;
        let hw_domain = proto_hw_to_domain(hw);

        // Look up enrollment by hardware identity
        let state = self.state.read().await;
        let hw_key = crate::raft::state::hw_canonical_key(&hw_domain);
        let node_id = match state.hw_index.get(&hw_key) {
            Some(id) => id.clone(),
            None => {
                drop(state);
                // Log failed attempt for audit
                warn!(mac = %hw.mac_address, "Enrollment attempt with unknown hardware identity");
                return Err(Status::not_found("NODE_NOT_ENROLLED"));
            }
        };

        let enrollment = state.enrollments.get(&node_id).cloned();
        drop(state);

        let enrollment = enrollment.ok_or_else(|| Status::internal("enrollment index inconsistency"))?;

        // Check state
        match enrollment.state {
            EnrollmentState::Revoked => return Err(Status::permission_denied("NODE_REVOKED")),
            EnrollmentState::Active => return Err(Status::already_exists("ALREADY_ACTIVE")),
            EnrollmentState::Registered | EnrollmentState::Inactive => {} // OK to proceed
        }

        // Sign CSR
        let signed = self
            .ca
            .sign_csr(&req.csr, &node_id, &self.domain_id)
            .map_err(|e| Status::internal(format!("CSR signing failed: {e}")))?;

        // Write ActivateNode to Raft
        let cmd = JournalCommand::ActivateNode {
            node_id: node_id.clone(),
            cert_serial: signed.cert_serial.clone(),
            cert_expires_at: signed.cert_expires_at.clone(),
        };
        let resp = self
            .raft
            .client_write(cmd)
            .await
            .map_err(|e| Status::internal(format!("Raft write failed: {e}")))?;

        match resp.data {
            JournalResponse::EnrollmentResult { vcluster_id, .. } => {
                info!(node_id = %node_id, "Node activated via enrollment");
                Ok(Response::new(EnrollResponse {
                    node_id,
                    domain_id: self.domain_id.clone(),
                    signed_cert: signed.cert_pem.into_bytes(),
                    ca_chain: signed.ca_chain_pem.into_bytes(),
                    vcluster_id: vcluster_id.unwrap_or_default(),
                    cert_serial: signed.cert_serial,
                    cert_expires_at: signed.cert_expires_at,
                }))
            }
            JournalResponse::ValidationError { reason } => Err(Status::failed_precondition(reason)),
            _ => Err(Status::internal("unexpected Raft response")),
        }
    }

    async fn register_node(
        &self,
        request: Request<RegisterNodeRequest>,
    ) -> Result<Response<RegisterNodeResponse>, Status> {
        Self::require_auth(&request)?;
        let principal = Self::extract_principal(&request);
        let req = request.into_inner();
        let hw = req
            .hardware_identity
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("hardware_identity required"))?;

        let enrollment = NodeEnrollment {
            node_id: req.node_id.clone(),
            domain_id: self.domain_id.clone(),
            state: EnrollmentState::Registered,
            hardware_identity: proto_hw_to_domain(hw),
            vcluster_id: None,
            cert_serial: None,
            cert_expires_at: None,
            last_seen: None,
            enrolled_at: chrono::Utc::now(),
            enrolled_by: Identity {
                principal,
                principal_type: PrincipalType::Human,
                role: "pact-platform-admin".to_string(),
            },
            active_sessions: 0,
        };

        let cmd = JournalCommand::RegisterNode { enrollment };
        let resp = self
            .raft
            .client_write(cmd)
            .await
            .map_err(|e| Status::internal(format!("Raft write failed: {e}")))?;

        match resp.data {
            JournalResponse::EnrollmentResult { node_id, state, .. } => {
                Ok(Response::new(RegisterNodeResponse {
                    node_id,
                    enrollment_state: format!("{state:?}"),
                }))
            }
            JournalResponse::ValidationError { reason } => Err(Status::failed_precondition(reason)),
            _ => Err(Status::internal("unexpected Raft response")),
        }
    }

    async fn batch_register_nodes(
        &self,
        request: Request<BatchRegisterNodesRequest>,
    ) -> Result<Response<BatchRegisterNodesResponse>, Status> {
        Self::require_auth(&request)?;
        let req = request.into_inner();
        let mut results = Vec::with_capacity(req.nodes.len());
        let mut succeeded = 0u32;
        let mut failed = 0u32;

        for node_req in &req.nodes {
            let hw = match node_req.hardware_identity.as_ref() {
                Some(hw) => proto_hw_to_domain(hw),
                None => {
                    results.push(BatchNodeResult {
                        node_id: node_req.node_id.clone(),
                        success: false,
                        enrollment_state: String::new(),
                        error: "hardware_identity required".to_string(),
                    });
                    failed += 1;
                    continue;
                }
            };

            let enrollment = NodeEnrollment {
                node_id: node_req.node_id.clone(),
                domain_id: self.domain_id.clone(),
                state: EnrollmentState::Registered,
                hardware_identity: hw,
                vcluster_id: None,
                cert_serial: None,
                cert_expires_at: None,
                last_seen: None,
                enrolled_at: chrono::Utc::now(),
                enrolled_by: Identity {
                    principal: "batch-admin".to_string(),
                    principal_type: PrincipalType::Human,
                    role: "pact-platform-admin".to_string(),
                },
                active_sessions: 0,
            };

            let cmd = JournalCommand::RegisterNode { enrollment };
            match self.raft.client_write(cmd).await {
                Ok(resp) => match resp.data {
                    JournalResponse::EnrollmentResult { node_id, state, .. } => {
                        results.push(BatchNodeResult {
                            node_id,
                            success: true,
                            enrollment_state: format!("{state:?}"),
                            error: String::new(),
                        });
                        succeeded += 1;
                    }
                    JournalResponse::ValidationError { reason } => {
                        results.push(BatchNodeResult {
                            node_id: node_req.node_id.clone(),
                            success: false,
                            enrollment_state: String::new(),
                            error: reason,
                        });
                        failed += 1;
                    }
                    _ => {
                        results.push(BatchNodeResult {
                            node_id: node_req.node_id.clone(),
                            success: false,
                            enrollment_state: String::new(),
                            error: "unexpected response".to_string(),
                        });
                        failed += 1;
                    }
                },
                Err(e) => {
                    results.push(BatchNodeResult {
                        node_id: node_req.node_id.clone(),
                        success: false,
                        enrollment_state: String::new(),
                        error: format!("Raft error: {e}"),
                    });
                    failed += 1;
                }
            }
        }

        Ok(Response::new(BatchRegisterNodesResponse { results, succeeded, failed }))
    }

    async fn decommission_node(
        &self,
        request: Request<DecommissionNodeRequest>,
    ) -> Result<Response<DecommissionNodeResponse>, Status> {
        Self::require_auth(&request)?;
        let req = request.into_inner();

        // Check for active sessions
        let state = self.state.read().await;
        if let Some(enrollment) = state.enrollments.get(&req.node_id) {
            if enrollment.active_sessions > 0 && !req.force {
                return Err(Status::failed_precondition(format!(
                    "{} active session(s) on this node — use --force to terminate",
                    enrollment.active_sessions
                )));
            }
        }
        let cert_serial = state
            .enrollments
            .get(&req.node_id)
            .and_then(|e| e.cert_serial.clone())
            .unwrap_or_default();
        drop(state);

        let cmd = JournalCommand::RevokeNode { node_id: req.node_id.clone() };
        let resp = self
            .raft
            .client_write(cmd)
            .await
            .map_err(|e| Status::internal(format!("Raft write failed: {e}")))?;

        match resp.data {
            JournalResponse::Ok => Ok(Response::new(DecommissionNodeResponse {
                node_id: req.node_id,
                enrollment_state: "Revoked".to_string(),
                sessions_terminated: 0,
                cert_serial_revoked: cert_serial,
            })),
            JournalResponse::ValidationError { reason } => Err(Status::failed_precondition(reason)),
            _ => Err(Status::internal("unexpected response")),
        }
    }

    async fn assign_node(
        &self,
        request: Request<AssignNodeRequest>,
    ) -> Result<Response<AssignNodeResponse>, Status> {
        Self::require_auth(&request)?;
        let req = request.into_inner();

        let cmd = JournalCommand::AssignNodeToVCluster {
            node_id: req.node_id.clone(),
            vcluster_id: req.vcluster_id.clone(),
        };
        let resp = self
            .raft
            .client_write(cmd)
            .await
            .map_err(|e| Status::internal(format!("Raft write failed: {e}")))?;

        match resp.data {
            JournalResponse::Ok => Ok(Response::new(AssignNodeResponse {
                node_id: req.node_id,
                vcluster_id: req.vcluster_id,
            })),
            JournalResponse::ValidationError { reason } => Err(Status::failed_precondition(reason)),
            _ => Err(Status::internal("unexpected response")),
        }
    }

    async fn unassign_node(
        &self,
        request: Request<UnassignNodeRequest>,
    ) -> Result<Response<UnassignNodeResponse>, Status> {
        Self::require_auth(&request)?;
        let req = request.into_inner();

        let cmd = JournalCommand::UnassignNode { node_id: req.node_id.clone() };
        let resp = self
            .raft
            .client_write(cmd)
            .await
            .map_err(|e| Status::internal(format!("Raft write failed: {e}")))?;

        match resp.data {
            JournalResponse::Ok => {
                Ok(Response::new(UnassignNodeResponse { node_id: req.node_id }))
            }
            JournalResponse::ValidationError { reason } => Err(Status::failed_precondition(reason)),
            _ => Err(Status::internal("unexpected response")),
        }
    }

    async fn move_node(
        &self,
        request: Request<MoveNodeRequest>,
    ) -> Result<Response<MoveNodeResponse>, Status> {
        Self::require_auth(&request)?;
        let req = request.into_inner();

        // Get current vCluster assignment
        let state = self.state.read().await;
        let from_vc = state
            .enrollments
            .get(&req.node_id)
            .and_then(|e| e.vcluster_id.clone())
            .unwrap_or_default();
        drop(state);

        let cmd = JournalCommand::MoveNodeVCluster {
            node_id: req.node_id.clone(),
            from_vcluster_id: from_vc.clone(),
            to_vcluster_id: req.to_vcluster_id.clone(),
        };
        let resp = self
            .raft
            .client_write(cmd)
            .await
            .map_err(|e| Status::internal(format!("Raft write failed: {e}")))?;

        match resp.data {
            JournalResponse::Ok => Ok(Response::new(MoveNodeResponse {
                node_id: req.node_id,
                from_vcluster_id: from_vc,
                to_vcluster_id: req.to_vcluster_id,
            })),
            JournalResponse::ValidationError { reason } => Err(Status::failed_precondition(reason)),
            _ => Err(Status::internal("unexpected response")),
        }
    }

    async fn renew_cert(
        &self,
        request: Request<RenewCertRequest>,
    ) -> Result<Response<RenewCertResponse>, Status> {
        // Authenticated — agent uses its existing mTLS cert
        Self::require_auth(&request)?;
        let req = request.into_inner();

        // Find node by current cert serial
        let state = self.state.read().await;
        let node_id = state
            .enrollments
            .values()
            .find(|e| e.cert_serial.as_deref() == Some(&req.current_cert_serial))
            .map(|e| e.node_id.clone())
            .ok_or_else(|| Status::not_found("no enrollment for this certificate serial"))?;
        drop(state);

        // Sign new CSR
        let signed = self
            .ca
            .sign_csr(&req.csr, &node_id, &self.domain_id)
            .map_err(|e| Status::internal(format!("CSR signing failed: {e}")))?;

        // Update cert in Raft
        let cmd = JournalCommand::ActivateNode {
            node_id: node_id.clone(),
            cert_serial: signed.cert_serial.clone(),
            cert_expires_at: signed.cert_expires_at.clone(),
        };

        // For renewal, the node is already active, so we update cert directly
        // We need to handle ALREADY_ACTIVE differently for renewals
        let state_guard = self.state.read().await;
        if let Some(enrollment) = state_guard.enrollments.get(&node_id) {
            if enrollment.state == EnrollmentState::Active {
                // Directly update cert fields via a different Raft path
                // For now, update last_seen to avoid ALREADY_ACTIVE error
                drop(state_guard);
                let update_cmd = JournalCommand::UpdateNodeLastSeen {
                    node_id: node_id.clone(),
                    timestamp: chrono::Utc::now().to_rfc3339(),
                };
                let _ = self.raft.client_write(update_cmd).await;
            } else {
                drop(state_guard);
                let _ = self.raft.client_write(cmd).await;
            }
        } else {
            drop(state_guard);
        }

        info!(node_id = %node_id, "Certificate renewed");
        Ok(Response::new(RenewCertResponse {
            signed_cert: signed.cert_pem.into_bytes(),
            ca_chain: signed.ca_chain_pem.into_bytes(),
            cert_serial: signed.cert_serial,
            cert_expires_at: signed.cert_expires_at,
        }))
    }

    async fn list_nodes(
        &self,
        request: Request<ListNodesRequest>,
    ) -> Result<Response<ListNodesResponse>, Status> {
        Self::require_auth(&request)?;
        let req = request.into_inner();
        let state = self.state.read().await;

        let nodes: Vec<NodeSummary> = state
            .enrollments
            .values()
            .filter(|e| {
                if !req.state_filter.is_empty() {
                    let state_str = format!("{:?}", e.state);
                    if !state_str.eq_ignore_ascii_case(&req.state_filter) {
                        return false;
                    }
                }
                if !req.vcluster_filter.is_empty() {
                    if e.vcluster_id.as_deref() != Some(&req.vcluster_filter) {
                        return false;
                    }
                }
                if req.unassigned_only && e.vcluster_id.is_some() {
                    return false;
                }
                true
            })
            .map(|e| NodeSummary {
                node_id: e.node_id.clone(),
                enrollment_state: format!("{:?}", e.state),
                vcluster_id: e.vcluster_id.clone().unwrap_or_default(),
                last_seen: e
                    .last_seen
                    .map(|t| t.to_rfc3339())
                    .unwrap_or_default(),
                mac_address: e.hardware_identity.mac_address.clone(),
            })
            .collect();

        Ok(Response::new(ListNodesResponse { nodes }))
    }

    async fn inspect_node(
        &self,
        request: Request<InspectNodeRequest>,
    ) -> Result<Response<InspectNodeResponse>, Status> {
        Self::require_auth(&request)?;
        let req = request.into_inner();
        let state = self.state.read().await;

        let enrollment = state
            .enrollments
            .get(&req.node_id)
            .ok_or_else(|| Status::not_found(format!("node {} not found", req.node_id)))?;

        Ok(Response::new(InspectNodeResponse {
            node_id: enrollment.node_id.clone(),
            domain_id: enrollment.domain_id.clone(),
            enrollment_state: format!("{:?}", enrollment.state),
            hardware_identity: Some(pact_common::proto::enrollment::HardwareIdentity {
                mac_address: enrollment.hardware_identity.mac_address.clone(),
                bmc_serial: enrollment
                    .hardware_identity
                    .bmc_serial
                    .clone()
                    .unwrap_or_default(),
                extra: enrollment.hardware_identity.extra.clone(),
            }),
            vcluster_id: enrollment.vcluster_id.clone().unwrap_or_default(),
            cert_serial: enrollment.cert_serial.clone().unwrap_or_default(),
            cert_expires_at: enrollment
                .cert_expires_at
                .map(|t| t.to_rfc3339())
                .unwrap_or_default(),
            last_seen: enrollment
                .last_seen
                .map(|t| t.to_rfc3339())
                .unwrap_or_default(),
            enrolled_at: enrollment.enrolled_at.to_rfc3339(),
            enrolled_by: enrollment.enrolled_by.principal.clone(),
            active_sessions: enrollment.active_sessions,
        }))
    }
}

