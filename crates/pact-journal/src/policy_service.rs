//! PolicyService gRPC implementation (hosted in journal per ADR-003).
//!
//! Evaluates policy using pact-policy RBAC engine (P1-P8).
//! GetEffectivePolicy reads from local state.
//! UpdatePolicy writes through Raft.

use std::sync::Arc;

use openraft::Raft;
use tokio::sync::RwLock;
use tonic::{Request, Response, Status};
use tracing::debug;

use pact_common::proto::policy::{
    policy_service_server::PolicyService, ApprovalRequired as ProtoApprovalRequired,
    DecideApprovalRequest, DecideApprovalResponse, GetPolicyRequest, ListApprovalsRequest,
    ListApprovalsResponse, PendingApprovalEntry, PolicyEvalRequest, PolicyEvalResponse,
    UpdatePolicyRequest, UpdatePolicyResponse, VClusterPolicy as ProtoVClusterPolicy,
};
use pact_common::types::{ApprovalStatus, Identity, PendingApproval, PrincipalType, Scope};
use pact_policy::rbac::{RbacDecision, RbacEngine};
use pact_policy::rules::opa;

use crate::raft::types::{JournalCommand, JournalResponse, JournalTypeConfig};
use crate::JournalState;

/// gRPC PolicyService — hosted in journal process (ADR-003).
/// Uses pact-policy RBAC engine for real policy evaluation.
pub struct PolicyServiceImpl {
    raft: Raft<JournalTypeConfig>,
    state: Arc<RwLock<JournalState>>,
    opa_client: Option<Box<dyn opa::OpaClient>>,
}

impl PolicyServiceImpl {
    pub fn new(raft: Raft<JournalTypeConfig>, state: Arc<RwLock<JournalState>>) -> Self {
        Self { raft, state, opa_client: None }
    }

    /// Attach an OPA client for external policy evaluation on Defer.
    pub fn with_opa(mut self, client: Box<dyn opa::OpaClient>) -> Self {
        self.opa_client = Some(client);
        self
    }

    /// Convert proto Identity to domain Identity.
    fn proto_to_identity(proto: &pact_common::proto::config::Identity) -> Identity {
        let principal_type = match proto.principal_type.as_str() {
            "agent" => PrincipalType::Agent,
            "service" => PrincipalType::Service,
            _ => PrincipalType::Human,
        };
        Identity { principal: proto.principal.clone(), principal_type, role: proto.role.clone() }
    }

    /// Convert proto Scope to domain Scope.
    fn proto_to_scope(proto: &pact_common::proto::config::Scope) -> Scope {
        use pact_common::proto::config::scope::Scope as ProtoScope;
        match &proto.scope {
            Some(ProtoScope::NodeId(n)) => Scope::Node(n.clone()),
            Some(ProtoScope::VclusterId(vc)) => Scope::VCluster(vc.clone()),
            Some(ProtoScope::Global(true)) => Scope::Global,
            _ => Scope::Global,
        }
    }

    fn make_response(
        authorized: bool,
        policy_ref: String,
        denial_reason: Option<String>,
    ) -> PolicyEvalResponse {
        PolicyEvalResponse { authorized, policy_ref, denial_reason, approval: None }
    }

    fn policy_ref(
        policy: Option<&pact_common::types::VClusterPolicy>,
        prefix: &str,
        action: &str,
    ) -> String {
        policy.map_or_else(
            || format!("{prefix}:default:{action}"),
            |p| format!("{prefix}:{}:{action}", p.policy_id),
        )
    }

    /// Evaluate via OPA when RBAC defers and no two-person approval needed (ADR-003).
    async fn evaluate_opa(
        &self,
        identity: &Identity,
        scope: &Scope,
        action: &str,
        command: Option<&str>,
        policy: Option<&pact_common::types::VClusterPolicy>,
    ) -> PolicyEvalResponse {
        if let Some(ref opa_client) = self.opa_client {
            let opa_input = opa::OpaInput {
                identity: opa::OpaIdentity {
                    principal: identity.principal.clone(),
                    role: identity.role.clone(),
                    principal_type: format!("{:?}", identity.principal_type),
                },
                action: action.to_string(),
                scope: opa::OpaScope {
                    vcluster: match scope {
                        Scope::VCluster(vc) | Scope::Node(vc) => vc.clone(),
                        Scope::Global => String::new(),
                    },
                },
                command: command.map(String::from),
            };
            match opa_client.evaluate(&opa_input).await {
                Ok(opa::OpaDecision::Allow) => {
                    Self::make_response(true, Self::policy_ref(policy, "opa", action), None)
                }
                Ok(opa::OpaDecision::Deny { reason }) => {
                    Self::make_response(false, format!("opa:denied:{action}"), Some(reason))
                }
                Err(e) => {
                    tracing::warn!(error = %e, "OPA evaluation failed, degraded mode allow");
                    Self::make_response(true, Self::policy_ref(policy, "degraded", action), None)
                }
            }
        } else {
            // No OPA configured — allow (RBAC deferred, no complex rules)
            Self::make_response(true, Self::policy_ref(policy, "rbac", action), None)
        }
    }
}

#[tonic::async_trait]
impl PolicyService for PolicyServiceImpl {
    /// Evaluate policy using RBAC engine (P1-P8 invariants).
    async fn evaluate(
        &self,
        request: Request<PolicyEvalRequest>,
    ) -> Result<Response<PolicyEvalResponse>, Status> {
        let req = request.into_inner();

        // Extract identity and scope from request
        let identity = req.author.as_ref().map_or_else(
            || Identity {
                principal: "anonymous".into(),
                principal_type: PrincipalType::Human,
                role: String::new(),
            },
            Self::proto_to_identity,
        );

        let scope = req.scope.as_ref().map_or(Scope::Global, Self::proto_to_scope);

        // Look up the vCluster policy for two-person approval check
        let state = self.state.read().await;
        let vcluster_policy = match &scope {
            Scope::VCluster(vc) => state.policies.get(vc).cloned(),
            _ => None,
        };
        drop(state);

        let default_policy = pact_common::types::VClusterPolicy::default();
        let policy = vcluster_policy.as_ref().unwrap_or(&default_policy);

        // Run RBAC evaluation
        let rbac_decision = RbacEngine::evaluate(&identity, &req.action, &scope, policy);

        debug!(
            principal = %identity.principal,
            action = %req.action,
            decision = ?rbac_decision,
            "Policy evaluation"
        );

        match rbac_decision {
            RbacDecision::Allow => {
                let policy_ref = Self::policy_ref(vcluster_policy.as_ref(), "rbac", &req.action);
                Ok(Response::new(Self::make_response(true, policy_ref, None)))
            }
            RbacDecision::Deny { reason } => Ok(Response::new(Self::make_response(
                false,
                format!("rbac:denied:{}", req.action),
                Some(reason),
            ))),
            RbacDecision::Defer => {
                if policy.two_person_approval {
                    // Two-person approval needed — persist through Raft
                    let approval_id = uuid::Uuid::new_v4().to_string();
                    let now = chrono::Utc::now();
                    let approval = PendingApproval {
                        approval_id: approval_id.clone(),
                        original_request: req.command.clone().unwrap_or_else(|| req.action.clone()),
                        action: req.action.clone(),
                        scope: scope.clone(),
                        requester: identity,
                        approver: None,
                        status: ApprovalStatus::Pending,
                        created_at: now,
                        expires_at: now + chrono::Duration::hours(24),
                    };
                    self.raft
                        .client_write(JournalCommand::CreateApproval(approval))
                        .await
                        .map_err(|e| Status::internal(format!("Raft write failed: {e}")))?;

                    let mut resp =
                        Self::make_response(false, format!("rbac:deferred:{}", req.action), None);
                    resp.approval = Some(ProtoApprovalRequired {
                        approval_type: "two_person".into(),
                        pending_approval_id: approval_id,
                    });
                    Ok(Response::new(resp))
                } else {
                    // Defer to OPA (or allow if no OPA configured)
                    let resp = self
                        .evaluate_opa(
                            &identity,
                            &scope,
                            &req.action,
                            req.command.as_deref(),
                            vcluster_policy.as_ref(),
                        )
                        .await;
                    Ok(Response::new(resp))
                }
            }
        }
    }

    /// Read effective policy for a vCluster from local state (J8).
    async fn get_effective_policy(
        &self,
        request: Request<GetPolicyRequest>,
    ) -> Result<Response<ProtoVClusterPolicy>, Status> {
        let vcluster_id = request.into_inner().vcluster_id;
        let state = self.state.read().await;
        let policy = state
            .policies
            .get(&vcluster_id)
            .ok_or_else(|| Status::not_found(format!("policy for {vcluster_id} not found")))?;
        Ok(Response::new(vcluster_policy_to_proto(policy)))
    }

    /// Write policy update through Raft consensus (J7).
    async fn update_policy(
        &self,
        request: Request<UpdatePolicyRequest>,
    ) -> Result<Response<UpdatePolicyResponse>, Status> {
        let req = request.into_inner();
        let vcluster_id = req.vcluster_id.clone();
        let proto_policy = req.policy.ok_or_else(|| Status::invalid_argument("policy required"))?;

        let policy = proto_to_vcluster_policy(proto_policy);

        let cmd = JournalCommand::SetPolicy { vcluster_id: vcluster_id.clone(), policy };
        let resp = self
            .raft
            .client_write(cmd)
            .await
            .map_err(|e| Status::internal(format!("Raft write failed: {e}")))?;

        match resp.data {
            JournalResponse::Ok => Ok(Response::new(UpdatePolicyResponse {
                success: true,
                policy_ref: format!("policy:{vcluster_id}"),
                error: None,
            })),
            JournalResponse::ValidationError { reason } => {
                Ok(Response::new(UpdatePolicyResponse {
                    success: false,
                    policy_ref: String::new(),
                    error: Some(reason),
                }))
            }
            _ => Err(Status::internal("unexpected response for UpdatePolicy")),
        }
    }

    /// List pending approval requests from local state (J8).
    async fn list_pending_approvals(
        &self,
        request: Request<ListApprovalsRequest>,
    ) -> Result<Response<ListApprovalsResponse>, Status> {
        let req = request.into_inner();
        let state = self.state.read().await;

        let approvals: Vec<PendingApprovalEntry> = state
            .pending_approvals
            .values()
            .filter(|a| {
                // Optionally filter by scope
                if let Some(ref filter) = req.scope_filter {
                    match &a.scope {
                        Scope::VCluster(vc) => vc == filter,
                        _ => filter.is_empty(),
                    }
                } else {
                    true
                }
            })
            .map(|a| {
                let scope_str = match &a.scope {
                    Scope::Global => "global".to_string(),
                    Scope::VCluster(vc) => format!("vc:{vc}"),
                    Scope::Node(n) => format!("node:{n}"),
                };
                PendingApprovalEntry {
                    approval_id: a.approval_id.clone(),
                    action: a.action.clone(),
                    scope: scope_str,
                    requester: a.requester.principal.clone(),
                    status: format!("{:?}", a.status),
                    created_at: Some(prost_types::Timestamp {
                        seconds: a.created_at.timestamp(),
                        nanos: 0,
                    }),
                    expires_at: Some(prost_types::Timestamp {
                        seconds: a.expires_at.timestamp(),
                        nanos: 0,
                    }),
                }
            })
            .collect();

        Ok(Response::new(ListApprovalsResponse { approvals }))
    }

    /// Decide on a pending approval through Raft (J7).
    async fn decide_approval(
        &self,
        request: Request<DecideApprovalRequest>,
    ) -> Result<Response<DecideApprovalResponse>, Status> {
        let req = request.into_inner();

        let approver_proto =
            req.approver.ok_or_else(|| Status::invalid_argument("approver identity required"))?;
        let approver = Self::proto_to_identity(&approver_proto);

        let decision = match req.decision.as_str() {
            "approved" => ApprovalStatus::Approved,
            "rejected" => ApprovalStatus::Rejected,
            other => {
                return Err(Status::invalid_argument(format!(
                    "invalid decision '{other}', must be 'approved' or 'rejected'"
                )))
            }
        };

        let cmd = JournalCommand::DecideApproval {
            approval_id: req.approval_id.clone(),
            approver,
            decision,
        };
        let resp = self
            .raft
            .client_write(cmd)
            .await
            .map_err(|e| Status::internal(format!("Raft write failed: {e}")))?;

        match resp.data {
            JournalResponse::Ok => {
                Ok(Response::new(DecideApprovalResponse { success: true, error: None }))
            }
            JournalResponse::ValidationError { reason } => {
                Ok(Response::new(DecideApprovalResponse { success: false, error: Some(reason) }))
            }
            _ => Err(Status::internal("unexpected response for DecideApproval")),
        }
    }
}

/// Convert domain VClusterPolicy to proto VClusterPolicy.
pub fn vcluster_policy_to_proto(
    policy: &pact_common::types::VClusterPolicy,
) -> ProtoVClusterPolicy {
    use pact_common::proto::policy::RoleBinding as ProtoRoleBinding;

    let updated_at = policy.updated_at.map(|dt| prost_types::Timestamp {
        seconds: dt.timestamp(),
        nanos: dt.timestamp_subsec_nanos() as i32,
    });

    let role_bindings = policy
        .role_bindings
        .iter()
        .map(|rb| ProtoRoleBinding {
            role: rb.role.clone(),
            principals: rb.principals.clone(),
            allowed_actions: rb.allowed_actions.clone(),
        })
        .collect();

    ProtoVClusterPolicy {
        vcluster_id: policy.vcluster_id.clone(),
        policy_id: policy.policy_id.clone(),
        updated_at,
        drift_sensitivity: policy.drift_sensitivity,
        base_commit_window_seconds: policy.base_commit_window_seconds,
        emergency_window_seconds: policy.emergency_window_seconds,
        auto_converge_categories: policy.auto_converge_categories.clone(),
        require_ack_categories: policy.require_ack_categories.clone(),
        enforcement_mode: policy.enforcement_mode.clone(),
        role_bindings,
        regulated: policy.regulated,
        two_person_approval: policy.two_person_approval,
        audit_retention_days: policy.audit_retention_days,
        federation_template: policy.federation_template.clone(),
        supervisor_backend: policy.supervisor_backend.clone(),
        exec_whitelist: policy.exec_whitelist.clone(),
        shell_whitelist: policy.shell_whitelist.clone(),
        emergency_allowed: policy.emergency_allowed,
        ai_exec_allowed: policy.ai_exec_allowed,
    }
}

/// Convert proto VClusterPolicy to domain VClusterPolicy.
pub fn proto_to_vcluster_policy(proto: ProtoVClusterPolicy) -> pact_common::types::VClusterPolicy {
    use pact_common::types::RoleBinding;

    let role_bindings = proto
        .role_bindings
        .into_iter()
        .map(|rb| RoleBinding {
            role: rb.role,
            principals: rb.principals,
            allowed_actions: rb.allowed_actions,
        })
        .collect();

    pact_common::types::VClusterPolicy {
        vcluster_id: proto.vcluster_id,
        policy_id: proto.policy_id,
        updated_at: proto.updated_at.map(|ts| {
            chrono::DateTime::from_timestamp(ts.seconds, ts.nanos as u32).unwrap_or_default()
        }),
        drift_sensitivity: proto.drift_sensitivity,
        base_commit_window_seconds: proto.base_commit_window_seconds,
        emergency_window_seconds: proto.emergency_window_seconds,
        auto_converge_categories: proto.auto_converge_categories,
        require_ack_categories: proto.require_ack_categories,
        enforcement_mode: proto.enforcement_mode,
        role_bindings,
        regulated: proto.regulated,
        two_person_approval: proto.two_person_approval,
        emergency_allowed: proto.emergency_allowed,
        audit_retention_days: proto.audit_retention_days,
        federation_template: proto.federation_template,
        supervisor_backend: proto.supervisor_backend,
        ai_exec_allowed: proto.ai_exec_allowed, // proto3 default: false
        exec_whitelist: proto.exec_whitelist,
        shell_whitelist: proto.shell_whitelist,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use openraft::impls::BasicNode;
    use openraft::Raft;
    use pact_common::proto::config::{
        scope::Scope as ProtoScopeInner, Identity as ProtoIdentity, Scope as ProtoScope,
    };
    use pact_common::types::{RoleBinding, VClusterPolicy};
    use raft_hpc_core::{FileLogStore, GrpcNetworkFactory, HpcStateMachine, StateMachineState};

    use crate::raft::types::JournalCommand;

    async fn test_policy_service() -> (PolicyServiceImpl, tempfile::TempDir) {
        let mut journal_state = JournalState::default();
        // Pre-populate with a policy
        journal_state.apply(JournalCommand::SetPolicy {
            vcluster_id: "ml-training".into(),
            policy: VClusterPolicy {
                vcluster_id: "ml-training".into(),
                policy_id: "pol-001".into(),
                drift_sensitivity: 3.0,
                base_commit_window_seconds: 1800,
                regulated: true,
                two_person_approval: true,
                exec_whitelist: vec!["nvidia-smi".into(), "dmesg".into()],
                shell_whitelist: vec!["ls".into(), "cat".into()],
                ..VClusterPolicy::default()
            },
        });

        let state = Arc::new(RwLock::new(journal_state));
        let temp = tempfile::tempdir().unwrap();
        let config = Arc::new(
            openraft::Config {
                heartbeat_interval: 500,
                election_timeout_min: 1500,
                election_timeout_max: 3000,
                ..Default::default()
            }
            .validate()
            .unwrap(),
        );
        let log_store = FileLogStore::<JournalTypeConfig>::new(temp.path()).unwrap();
        let snapshot_dir = temp.path().join("snapshots");
        std::fs::create_dir_all(&snapshot_dir).unwrap();
        let sm = HpcStateMachine::with_snapshot_dir(Arc::clone(&state), snapshot_dir).unwrap();
        let network = GrpcNetworkFactory::new();
        let raft = Raft::new(1, config, network, log_store, sm).await.unwrap();

        // Bootstrap as single-node cluster so Raft can elect a leader
        let mut members = std::collections::BTreeMap::new();
        members.insert(1, BasicNode::new("127.0.0.1:0".to_string()));
        raft.initialize(members).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        let svc = PolicyServiceImpl::new(raft, state);
        (svc, temp)
    }

    fn admin_identity() -> Option<ProtoIdentity> {
        Some(ProtoIdentity {
            principal: "admin@example.com".into(),
            principal_type: "admin".into(),
            role: "pact-platform-admin".into(),
        })
    }

    fn ops_identity(vcluster: &str) -> Option<ProtoIdentity> {
        Some(ProtoIdentity {
            principal: "ops@example.com".into(),
            principal_type: "admin".into(),
            role: format!("pact-ops-{vcluster}"),
        })
    }

    fn vcluster_scope(vc: &str) -> Option<ProtoScope> {
        Some(ProtoScope { scope: Some(ProtoScopeInner::VclusterId(vc.into())) })
    }

    // --- RBAC evaluate tests ---

    #[tokio::test]
    async fn evaluate_admin_allowed() {
        let (svc, _tmp) = test_policy_service().await;
        let resp = svc
            .evaluate(Request::new(PolicyEvalRequest {
                author: admin_identity(),
                scope: vcluster_scope("ml-training"),
                action: "commit".into(),
                proposed_change: None,
                command: None,
            }))
            .await
            .unwrap();
        let eval = resp.into_inner();
        assert!(eval.authorized);
        assert!(eval.policy_ref.contains("commit"));
        assert!(eval.denial_reason.is_none());
    }

    #[tokio::test]
    async fn evaluate_ops_allowed_on_own_vcluster() {
        let (svc, _tmp) = test_policy_service().await;
        let resp = svc
            .evaluate(Request::new(PolicyEvalRequest {
                author: ops_identity("ml-training"),
                scope: vcluster_scope("ml-training"),
                action: "exec".into(),
                proposed_change: None,
                command: Some("nvidia-smi".into()),
            }))
            .await
            .unwrap();
        let eval = resp.into_inner();
        // ml-training has two_person_approval=true and regulated=true,
        // but exec is not a state-changing action for regulated roles
        // so ops should be allowed
        assert!(eval.authorized);
        assert!(eval.policy_ref.contains("exec"));
    }

    #[tokio::test]
    async fn evaluate_ops_denied_wrong_vcluster() {
        let (svc, _tmp) = test_policy_service().await;
        let resp = svc
            .evaluate(Request::new(PolicyEvalRequest {
                author: ops_identity("other-vc"),
                scope: vcluster_scope("ml-training"),
                action: "commit".into(),
                proposed_change: None,
                command: None,
            }))
            .await
            .unwrap();
        let eval = resp.into_inner();
        assert!(!eval.authorized);
        assert!(eval.denial_reason.is_some());
    }

    #[tokio::test]
    async fn evaluate_anonymous_denied() {
        let (svc, _tmp) = test_policy_service().await;
        let resp = svc
            .evaluate(Request::new(PolicyEvalRequest {
                author: None,
                scope: None,
                action: "commit".into(),
                proposed_change: None,
                command: None,
            }))
            .await
            .unwrap();
        let eval = resp.into_inner();
        // Anonymous with empty role should be denied
        assert!(!eval.authorized);
    }

    #[tokio::test]
    async fn evaluate_regulated_defers_for_approval() {
        let (svc, _tmp) = test_policy_service().await;
        let resp = svc
            .evaluate(Request::new(PolicyEvalRequest {
                author: Some(ProtoIdentity {
                    principal: "regulated@example.com".into(),
                    principal_type: "admin".into(),
                    role: "pact-regulated-ml-training".into(),
                }),
                scope: vcluster_scope("ml-training"),
                action: "commit".into(),
                proposed_change: None,
                command: None,
            }))
            .await
            .unwrap();
        let eval = resp.into_inner();
        assert!(!eval.authorized);
        assert!(eval.approval.is_some());
        let approval = eval.approval.unwrap();
        assert_eq!(approval.approval_type, "two_person");
        assert!(!approval.pending_approval_id.is_empty());
    }

    #[tokio::test]
    async fn evaluate_includes_action_in_policy_ref() {
        let (svc, _tmp) = test_policy_service().await;
        let resp = svc
            .evaluate(Request::new(PolicyEvalRequest {
                author: admin_identity(),
                scope: vcluster_scope("ml-training"),
                action: "exec".into(),
                proposed_change: None,
                command: Some("nvidia-smi".into()),
            }))
            .await
            .unwrap();
        assert!(resp.into_inner().policy_ref.contains("exec"));
    }

    // --- GetEffectivePolicy tests ---

    #[tokio::test]
    async fn get_effective_policy_returns_stored_policy() {
        let (svc, _tmp) = test_policy_service().await;
        let resp = svc
            .get_effective_policy(Request::new(GetPolicyRequest {
                vcluster_id: "ml-training".into(),
            }))
            .await
            .unwrap();
        let policy = resp.into_inner();
        assert_eq!(policy.vcluster_id, "ml-training");
        assert_eq!(policy.policy_id, "pol-001");
        assert_eq!(policy.drift_sensitivity, 3.0);
        assert_eq!(policy.base_commit_window_seconds, 1800);
        assert!(policy.regulated);
        assert!(policy.two_person_approval);
        assert_eq!(policy.exec_whitelist, vec!["nvidia-smi", "dmesg"]);
        assert_eq!(policy.shell_whitelist, vec!["ls", "cat"]);
    }

    #[tokio::test]
    async fn get_effective_policy_not_found() {
        let (svc, _tmp) = test_policy_service().await;
        let result = svc
            .get_effective_policy(Request::new(GetPolicyRequest {
                vcluster_id: "nonexistent".into(),
            }))
            .await;
        assert_eq!(result.unwrap_err().code(), tonic::Code::NotFound);
    }

    // --- Proto conversion tests ---

    #[test]
    fn vcluster_policy_roundtrip() {
        let policy = VClusterPolicy {
            vcluster_id: "test-vc".into(),
            policy_id: "pol-42".into(),
            updated_at: Some(Utc::now()),
            drift_sensitivity: 5.0,
            base_commit_window_seconds: 600,
            emergency_window_seconds: 7200,
            auto_converge_categories: vec!["ntp".into()],
            require_ack_categories: vec!["kernel".into()],
            enforcement_mode: "enforce".into(),
            role_bindings: vec![RoleBinding {
                role: "pact-ops-test-vc".into(),
                principals: vec!["alice@example.com".into()],
                allowed_actions: vec!["commit".into(), "exec".into()],
            }],
            regulated: true,
            two_person_approval: true,
            emergency_allowed: false,
            audit_retention_days: 365,
            ai_exec_allowed: true,
            federation_template: Some("template-eu".into()),
            supervisor_backend: "systemd".into(),
            exec_whitelist: vec!["nvidia-smi".into()],
            shell_whitelist: vec!["ls".into()],
        };

        let proto = vcluster_policy_to_proto(&policy);
        let back = proto_to_vcluster_policy(proto);

        assert_eq!(back.vcluster_id, policy.vcluster_id);
        assert_eq!(back.policy_id, policy.policy_id);
        assert_eq!(back.drift_sensitivity, policy.drift_sensitivity);
        assert_eq!(back.base_commit_window_seconds, policy.base_commit_window_seconds);
        assert_eq!(back.emergency_window_seconds, policy.emergency_window_seconds);
        assert_eq!(back.enforcement_mode, policy.enforcement_mode);
        assert_eq!(back.regulated, policy.regulated);
        assert_eq!(back.two_person_approval, policy.two_person_approval);
        assert_eq!(back.emergency_allowed, policy.emergency_allowed);
        assert_eq!(back.audit_retention_days, policy.audit_retention_days);
        assert_eq!(back.federation_template, policy.federation_template);
        assert_eq!(back.supervisor_backend, policy.supervisor_backend);
        assert_eq!(back.exec_whitelist, policy.exec_whitelist);
        assert_eq!(back.shell_whitelist, policy.shell_whitelist);
        assert_eq!(back.role_bindings.len(), 1);
        assert_eq!(back.role_bindings[0].role, "pact-ops-test-vc");
    }
}
