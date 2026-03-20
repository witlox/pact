//! Policy evaluation engine — main entry point for PolicyService.
//!
//! Evaluation flow:
//! 1. RBAC check (fast, local) → Allow/Deny/Defer
//! 2. If Defer: OPA evaluation (feature-gated) or approval workflow
//! 3. Policy cache for degraded mode (P7)
//! 4. Two-person approval workflow (P4)

pub mod opa;

use std::collections::HashMap;

use async_trait::async_trait;
use chrono::{Duration, Utc};
use tracing::{info, warn};
use uuid::Uuid;

use pact_common::types::{ApprovalStatus, Identity, PendingApproval, Scope, VClusterPolicy};

use crate::rbac::{RbacDecision, RbacEngine};

/// Policy evaluation request.
#[derive(Debug, Clone)]
pub struct PolicyRequest {
    pub identity: Identity,
    pub scope: Scope,
    pub action: String,
    pub proposed_change: Option<pact_common::types::StateDelta>,
    pub command: Option<String>,
}

/// Policy evaluation result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    /// Operation is allowed.
    Allow { policy_ref: String },
    /// Operation is denied.
    Deny { policy_ref: String, reason: String },
    /// Operation requires two-person approval.
    RequireApproval { policy_ref: String, approval_id: String },
}

/// Trait for the policy evaluation engine.
#[async_trait]
pub trait PolicyEngine: Send + Sync {
    /// Evaluate a policy request.
    async fn evaluate(&self, request: &PolicyRequest) -> Result<PolicyDecision, PolicyError>;

    /// Get the effective policy for a vCluster.
    async fn get_effective_policy(&self, vcluster_id: &str) -> Result<VClusterPolicy, PolicyError>;
}

/// Default policy engine using RBAC + policy cache.
pub struct DefaultPolicyEngine {
    /// Cached policies per vCluster.
    policies: HashMap<String, VClusterPolicy>,
    /// Pending approval requests.
    pending_approvals: HashMap<String, PendingApproval>,
    /// Default approval timeout in seconds.
    approval_timeout_seconds: i64,
    /// Optional OPA client for external policy evaluation.
    opa_client: Option<Box<dyn opa::OpaClient>>,
}

impl DefaultPolicyEngine {
    pub fn new(approval_timeout_seconds: i64) -> Self {
        Self {
            policies: HashMap::new(),
            pending_approvals: HashMap::new(),
            approval_timeout_seconds,
            opa_client: None,
        }
    }

    /// Attach an OPA client for external policy evaluation on Defer.
    pub fn with_opa(mut self, client: Box<dyn opa::OpaClient>) -> Self {
        self.opa_client = Some(client);
        self
    }

    /// Load or update a vCluster policy.
    pub fn set_policy(&mut self, policy: VClusterPolicy) {
        info!(vcluster = %policy.vcluster_id, "Policy updated");
        self.policies.insert(policy.vcluster_id.clone(), policy);
    }

    /// Get a cached policy.
    pub fn get_policy(&self, vcluster_id: &str) -> Option<&VClusterPolicy> {
        self.policies.get(vcluster_id)
    }

    /// Evaluate a policy request (synchronous core logic).
    pub fn evaluate_sync(
        &mut self,
        request: &PolicyRequest,
    ) -> Result<PolicyDecision, PolicyError> {
        let vcluster_id = match &request.scope {
            Scope::VCluster(vc) | Scope::Node(vc) => vc.clone(),
            Scope::Global => String::new(),
        };

        let policy = self.policies.get(&vcluster_id).cloned().unwrap_or_else(|| VClusterPolicy {
            vcluster_id: vcluster_id.clone(),
            ..VClusterPolicy::default()
        });

        let policy_ref = if policy.policy_id.is_empty() {
            format!("default:{vcluster_id}")
        } else {
            policy.policy_id.clone()
        };

        // Step 1: RBAC check
        let rbac_result =
            RbacEngine::evaluate(&request.identity, &request.action, &request.scope, &policy);

        match rbac_result {
            RbacDecision::Allow => Ok(PolicyDecision::Allow { policy_ref }),
            RbacDecision::Deny { reason } => Ok(PolicyDecision::Deny { policy_ref, reason }),
            RbacDecision::Defer => {
                // Step 2: Two-person approval workflow
                if policy.two_person_approval {
                    let approval_id = self.create_approval(request, &vcluster_id);
                    Ok(PolicyDecision::RequireApproval { policy_ref, approval_id })
                } else {
                    // No two-person approval required, allow
                    Ok(PolicyDecision::Allow { policy_ref })
                }
            }
        }
    }

    /// Create a pending approval request.
    fn create_approval(&mut self, request: &PolicyRequest, vcluster_id: &str) -> String {
        let approval_id = Uuid::new_v4().to_string();
        let now = Utc::now();

        let approval = PendingApproval {
            approval_id: approval_id.clone(),
            original_request: format!("{} on {vcluster_id}", request.action),
            action: request.action.clone(),
            scope: request.scope.clone(),
            requester: request.identity.clone(),
            approver: None,
            status: ApprovalStatus::Pending,
            created_at: now,
            expires_at: now + Duration::seconds(self.approval_timeout_seconds),
        };

        info!(
            approval_id = %approval_id,
            action = %request.action,
            requester = %request.identity.principal,
            "Created pending approval"
        );

        self.pending_approvals.insert(approval_id.clone(), approval);
        approval_id
    }

    /// Approve a pending request. Returns error if same requester tries to approve (P4).
    pub fn approve(
        &mut self,
        approval_id: &str,
        approver: &Identity,
    ) -> Result<PolicyDecision, PolicyError> {
        let approval = self
            .pending_approvals
            .get_mut(approval_id)
            .ok_or_else(|| PolicyError::ApprovalNotFound(approval_id.into()))?;

        // Check not expired (P5)
        if Utc::now() > approval.expires_at {
            approval.status = ApprovalStatus::Expired;
            return Err(PolicyError::ApprovalExpired(approval_id.into()));
        }

        // Check not already resolved
        if approval.status != ApprovalStatus::Pending {
            return Err(PolicyError::ApprovalAlreadyResolved(approval_id.into()));
        }

        // P4: Same admin cannot approve their own request
        if approval.requester.principal == approver.principal {
            return Err(PolicyError::SelfApproval);
        }

        approval.status = ApprovalStatus::Approved;
        approval.approver = Some(approver.clone());

        info!(
            approval_id = %approval_id,
            approver = %approver.principal,
            action = %approval.action,
            "Approval granted"
        );

        Ok(PolicyDecision::Allow { policy_ref: format!("approved:{approval_id}") })
    }

    /// Reject a pending approval.
    pub fn reject(&mut self, approval_id: &str, rejector: &Identity) -> Result<(), PolicyError> {
        let approval = self
            .pending_approvals
            .get_mut(approval_id)
            .ok_or_else(|| PolicyError::ApprovalNotFound(approval_id.into()))?;

        if approval.status != ApprovalStatus::Pending {
            return Err(PolicyError::ApprovalAlreadyResolved(approval_id.into()));
        }

        approval.status = ApprovalStatus::Rejected;
        approval.approver = Some(rejector.clone());
        Ok(())
    }

    /// Expire all timed-out approvals (P5).
    pub fn expire_approvals(&mut self) -> Vec<String> {
        let now = Utc::now();
        let mut expired = Vec::new();

        for (id, approval) in &mut self.pending_approvals {
            if approval.status == ApprovalStatus::Pending && now > approval.expires_at {
                approval.status = ApprovalStatus::Expired;
                expired.push(id.clone());
                warn!(approval_id = %id, "Approval expired (P5)");
            }
        }

        expired
    }

    /// Get all pending approvals.
    pub fn pending_approvals(&self) -> Vec<&PendingApproval> {
        self.pending_approvals.values().filter(|a| a.status == ApprovalStatus::Pending).collect()
    }

    /// Get a specific approval.
    pub fn get_approval(&self, approval_id: &str) -> Option<&PendingApproval> {
        self.pending_approvals.get(approval_id)
    }

    /// Clean up resolved approvals.
    pub fn cleanup_resolved(&mut self) {
        self.pending_approvals.retain(|_, a| a.status == ApprovalStatus::Pending);
    }
}

#[async_trait]
impl PolicyEngine for DefaultPolicyEngine {
    async fn evaluate(&self, request: &PolicyRequest) -> Result<PolicyDecision, PolicyError> {
        // Async version performs RBAC evaluation without approval creation
        // (approvals require &mut self, which the async trait doesn't allow).
        // Production code uses RbacEngine::evaluate() directly via PolicyServiceImpl.
        let vcluster_id = match &request.scope {
            Scope::VCluster(vc) | Scope::Node(vc) => vc.clone(),
            Scope::Global => String::new(),
        };

        let policy = self.policies.get(&vcluster_id).cloned().unwrap_or_else(|| VClusterPolicy {
            vcluster_id: vcluster_id.clone(),
            ..VClusterPolicy::default()
        });

        let policy_ref = if policy.policy_id.is_empty() {
            format!("default:{vcluster_id}")
        } else {
            policy.policy_id.clone()
        };

        let rbac_result =
            RbacEngine::evaluate(&request.identity, &request.action, &request.scope, &policy);

        match rbac_result {
            RbacDecision::Allow => Ok(PolicyDecision::Allow { policy_ref }),
            RbacDecision::Deny { reason } => Ok(PolicyDecision::Deny { policy_ref, reason }),
            RbacDecision::Defer => {
                if policy.two_person_approval {
                    // Cannot create approval without &mut self — return RequireApproval
                    // with a placeholder. Production path uses evaluate_sync() or
                    // PolicyServiceImpl which handles approvals through Raft.
                    Ok(PolicyDecision::RequireApproval {
                        policy_ref,
                        approval_id: "pending".to_string(),
                    })
                } else if let Some(ref opa) = self.opa_client {
                    // OPA evaluation for complex rules (ADR-003)
                    let opa_input = opa::OpaInput::from_request(request);
                    match opa.evaluate(&opa_input).await {
                        Ok(opa::OpaDecision::Allow) => Ok(PolicyDecision::Allow { policy_ref }),
                        Ok(opa::OpaDecision::Deny { reason }) => {
                            Ok(PolicyDecision::Deny { policy_ref, reason })
                        }
                        Err(e) => {
                            // ADR-011: degraded mode — warn and allow
                            warn!(error = %e, "OPA evaluation failed, degraded mode allow");
                            Ok(PolicyDecision::Allow { policy_ref })
                        }
                    }
                } else {
                    Ok(PolicyDecision::Allow { policy_ref })
                }
            }
        }
    }

    async fn get_effective_policy(&self, vcluster_id: &str) -> Result<VClusterPolicy, PolicyError> {
        self.policies
            .get(vcluster_id)
            .cloned()
            .ok_or_else(|| PolicyError::PolicyNotFound(vcluster_id.into()))
    }
}

/// Policy evaluation errors.
#[derive(Debug, Clone, thiserror::Error)]
pub enum PolicyError {
    #[error("policy not found for vCluster: {0}")]
    PolicyNotFound(String),
    #[error("approval not found: {0}")]
    ApprovalNotFound(String),
    #[error("approval expired: {0}")]
    ApprovalExpired(String),
    #[error("approval already resolved: {0}")]
    ApprovalAlreadyResolved(String),
    #[error("requester cannot approve their own request (P4)")]
    SelfApproval,
    #[error("OPA evaluation failed: {0}")]
    OpaError(String),
    #[error("not implemented: {0}")]
    NotImplemented(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rbac::actions;
    use pact_common::types::{PrincipalType, Scope};

    fn admin() -> Identity {
        Identity {
            principal: "admin@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-platform-admin".into(),
        }
    }

    fn ops(vc: &str) -> Identity {
        Identity {
            principal: "ops@example.com".into(),
            principal_type: PrincipalType::Human,
            role: format!("pact-ops-{vc}"),
        }
    }

    fn regulated_user(vc: &str) -> Identity {
        Identity {
            principal: "regulated@example.com".into(),
            principal_type: PrincipalType::Human,
            role: format!("pact-regulated-{vc}"),
        }
    }

    fn second_admin() -> Identity {
        Identity {
            principal: "second-admin@example.com".into(),
            principal_type: PrincipalType::Human,
            role: "pact-ops-ml".into(),
        }
    }

    fn request(identity: Identity, action: &str, vc: &str) -> PolicyRequest {
        PolicyRequest {
            identity,
            scope: Scope::VCluster(vc.into()),
            action: action.into(),
            proposed_change: None,
            command: None,
        }
    }

    // --- Basic evaluation ---

    #[test]
    fn admin_allowed_for_any_action() {
        let mut engine = DefaultPolicyEngine::new(1800);
        engine.set_policy(VClusterPolicy { vcluster_id: "ml".into(), ..VClusterPolicy::default() });

        let req = request(admin(), actions::COMMIT, "ml");
        let result = engine.evaluate_sync(&req).unwrap();
        assert!(matches!(result, PolicyDecision::Allow { .. }));
    }

    #[test]
    fn ops_allowed_on_own_vcluster() {
        let mut engine = DefaultPolicyEngine::new(1800);
        engine.set_policy(VClusterPolicy { vcluster_id: "ml".into(), ..VClusterPolicy::default() });

        let req = request(ops("ml"), actions::EXEC, "ml");
        let result = engine.evaluate_sync(&req).unwrap();
        assert!(matches!(result, PolicyDecision::Allow { .. }));
    }

    #[test]
    fn ops_denied_on_wrong_vcluster() {
        let mut engine = DefaultPolicyEngine::new(1800);
        engine.set_policy(VClusterPolicy {
            vcluster_id: "other".into(),
            ..VClusterPolicy::default()
        });

        let req = request(ops("ml"), actions::EXEC, "other");
        let result = engine.evaluate_sync(&req).unwrap();
        assert!(matches!(result, PolicyDecision::Deny { .. }));
    }

    #[test]
    fn missing_policy_uses_default() {
        let mut engine = DefaultPolicyEngine::new(1800);
        // No policy set — should use default

        let req = request(ops("ml"), actions::EXEC, "ml");
        let result = engine.evaluate_sync(&req).unwrap();
        assert!(matches!(result, PolicyDecision::Allow { .. }));
    }

    // --- Two-person approval workflow ---

    #[test]
    fn regulated_role_triggers_approval() {
        let mut engine = DefaultPolicyEngine::new(1800);
        engine.set_policy(VClusterPolicy {
            vcluster_id: "ml".into(),
            two_person_approval: true,
            regulated: true,
            ..VClusterPolicy::default()
        });

        let req = request(regulated_user("ml"), actions::COMMIT, "ml");
        let result = engine.evaluate_sync(&req).unwrap();
        assert!(
            matches!(result, PolicyDecision::RequireApproval { .. }),
            "regulated role with two_person_approval should require approval"
        );
    }

    #[test]
    fn approval_workflow_full_cycle() {
        let mut engine = DefaultPolicyEngine::new(1800);
        engine.set_policy(VClusterPolicy {
            vcluster_id: "ml".into(),
            two_person_approval: true,
            regulated: true,
            ..VClusterPolicy::default()
        });

        // Step 1: Create approval
        let req = request(regulated_user("ml"), actions::COMMIT, "ml");
        let result = engine.evaluate_sync(&req).unwrap();
        let approval_id = match result {
            PolicyDecision::RequireApproval { approval_id, .. } => approval_id,
            other => panic!("expected RequireApproval, got {other:?}"),
        };

        // Verify pending
        assert_eq!(engine.pending_approvals().len(), 1);
        assert_eq!(engine.get_approval(&approval_id).unwrap().status, ApprovalStatus::Pending);

        // Step 2: Second admin approves
        let result = engine.approve(&approval_id, &second_admin()).unwrap();
        assert!(matches!(result, PolicyDecision::Allow { .. }));

        // Verify approved
        assert_eq!(engine.get_approval(&approval_id).unwrap().status, ApprovalStatus::Approved);
        assert_eq!(engine.pending_approvals().len(), 0);
    }

    #[test]
    fn self_approval_denied() {
        let mut engine = DefaultPolicyEngine::new(1800);
        engine.set_policy(VClusterPolicy {
            vcluster_id: "ml".into(),
            two_person_approval: true,
            regulated: true,
            ..VClusterPolicy::default()
        });

        let req = request(regulated_user("ml"), actions::COMMIT, "ml");
        let result = engine.evaluate_sync(&req).unwrap();
        let approval_id = match result {
            PolicyDecision::RequireApproval { approval_id, .. } => approval_id,
            other => panic!("expected RequireApproval, got {other:?}"),
        };

        // Same user tries to approve
        let result = engine.approve(&approval_id, &regulated_user("ml"));
        assert!(matches!(result, Err(PolicyError::SelfApproval)));
    }

    #[test]
    fn approval_expiry() {
        let mut engine = DefaultPolicyEngine::new(1); // 1 second timeout
        engine.set_policy(VClusterPolicy {
            vcluster_id: "ml".into(),
            two_person_approval: true,
            regulated: true,
            ..VClusterPolicy::default()
        });

        let req = request(regulated_user("ml"), actions::COMMIT, "ml");
        let result = engine.evaluate_sync(&req).unwrap();
        let approval_id = match result {
            PolicyDecision::RequireApproval { approval_id, .. } => approval_id,
            other => panic!("expected RequireApproval, got {other:?}"),
        };

        // Manually expire by backdating
        if let Some(approval) = engine.pending_approvals.get_mut(&approval_id) {
            approval.expires_at = Utc::now() - Duration::seconds(10);
        }

        // Try to approve — should fail
        let result = engine.approve(&approval_id, &second_admin());
        assert!(matches!(result, Err(PolicyError::ApprovalExpired(_))));
    }

    #[test]
    fn expire_approvals_batch() {
        let mut engine = DefaultPolicyEngine::new(1);
        engine.set_policy(VClusterPolicy {
            vcluster_id: "ml".into(),
            two_person_approval: true,
            regulated: true,
            ..VClusterPolicy::default()
        });

        // Create two approvals
        let req1 = request(regulated_user("ml"), actions::COMMIT, "ml");
        engine.evaluate_sync(&req1).unwrap();
        let req2 = request(regulated_user("ml"), actions::EXEC, "ml");
        engine.evaluate_sync(&req2).unwrap();

        assert_eq!(engine.pending_approvals().len(), 2);

        // Backdate all approvals
        for approval in engine.pending_approvals.values_mut() {
            approval.expires_at = Utc::now() - Duration::seconds(10);
        }

        let expired = engine.expire_approvals();
        assert_eq!(expired.len(), 2);
        assert_eq!(engine.pending_approvals().len(), 0);
    }

    #[test]
    fn reject_approval() {
        let mut engine = DefaultPolicyEngine::new(1800);
        engine.set_policy(VClusterPolicy {
            vcluster_id: "ml".into(),
            two_person_approval: true,
            regulated: true,
            ..VClusterPolicy::default()
        });

        let req = request(regulated_user("ml"), actions::COMMIT, "ml");
        let result = engine.evaluate_sync(&req).unwrap();
        let approval_id = match result {
            PolicyDecision::RequireApproval { approval_id, .. } => approval_id,
            other => panic!("expected RequireApproval, got {other:?}"),
        };

        engine.reject(&approval_id, &second_admin()).unwrap();
        assert_eq!(engine.get_approval(&approval_id).unwrap().status, ApprovalStatus::Rejected);
    }

    #[test]
    fn double_approve_fails() {
        let mut engine = DefaultPolicyEngine::new(1800);
        engine.set_policy(VClusterPolicy {
            vcluster_id: "ml".into(),
            two_person_approval: true,
            regulated: true,
            ..VClusterPolicy::default()
        });

        let req = request(regulated_user("ml"), actions::COMMIT, "ml");
        let result = engine.evaluate_sync(&req).unwrap();
        let approval_id = match result {
            PolicyDecision::RequireApproval { approval_id, .. } => approval_id,
            other => panic!("expected RequireApproval, got {other:?}"),
        };

        engine.approve(&approval_id, &second_admin()).unwrap();
        let result = engine.approve(&approval_id, &admin());
        assert!(matches!(result, Err(PolicyError::ApprovalAlreadyResolved(_))));
    }

    #[test]
    fn approve_nonexistent_fails() {
        let mut engine = DefaultPolicyEngine::new(1800);
        let result = engine.approve("nonexistent", &admin());
        assert!(matches!(result, Err(PolicyError::ApprovalNotFound(_))));
    }

    // --- Policy management ---

    #[test]
    fn set_and_get_policy() {
        let mut engine = DefaultPolicyEngine::new(1800);
        let policy = VClusterPolicy {
            vcluster_id: "ml".into(),
            policy_id: "pol-001".into(),
            drift_sensitivity: 3.0,
            ..VClusterPolicy::default()
        };
        engine.set_policy(policy);

        let cached = engine.get_policy("ml").unwrap();
        assert_eq!(cached.policy_id, "pol-001");
        assert!((cached.drift_sensitivity - 3.0).abs() < f64::EPSILON);
    }

    #[test]
    fn cleanup_resolved_keeps_pending() {
        let mut engine = DefaultPolicyEngine::new(1800);
        engine.set_policy(VClusterPolicy {
            vcluster_id: "ml".into(),
            two_person_approval: true,
            regulated: true,
            ..VClusterPolicy::default()
        });

        // Create and approve one
        let req1 = request(regulated_user("ml"), actions::COMMIT, "ml");
        let result = engine.evaluate_sync(&req1).unwrap();
        let id1 = match result {
            PolicyDecision::RequireApproval { approval_id, .. } => approval_id,
            _ => panic!(),
        };
        engine.approve(&id1, &second_admin()).unwrap();

        // Create another (still pending)
        let req2 = request(regulated_user("ml"), actions::EXEC, "ml");
        engine.evaluate_sync(&req2).unwrap();

        engine.cleanup_resolved();
        assert_eq!(engine.pending_approvals.len(), 1); // only pending one remains
    }

    // --- Policy ref ---

    #[test]
    fn policy_ref_uses_policy_id_when_set() {
        let mut engine = DefaultPolicyEngine::new(1800);
        engine.set_policy(VClusterPolicy {
            vcluster_id: "ml".into(),
            policy_id: "pol-123".into(),
            ..VClusterPolicy::default()
        });

        let req = request(admin(), actions::STATUS, "ml");
        let result = engine.evaluate_sync(&req).unwrap();
        match result {
            PolicyDecision::Allow { policy_ref } => assert_eq!(policy_ref, "pol-123"),
            other => panic!("expected Allow, got {other:?}"),
        }
    }

    #[test]
    fn policy_ref_uses_default_when_no_policy_id() {
        let mut engine = DefaultPolicyEngine::new(1800);
        // No policy set, uses default

        let req = request(admin(), actions::STATUS, "ml");
        let result = engine.evaluate_sync(&req).unwrap();
        match result {
            PolicyDecision::Allow { policy_ref } => assert_eq!(policy_ref, "default:ml"),
            other => panic!("expected Allow, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn get_effective_policy_found() {
        let mut engine = DefaultPolicyEngine::new(1800);
        engine.set_policy(VClusterPolicy {
            vcluster_id: "ml".into(),
            drift_sensitivity: 5.0,
            ..VClusterPolicy::default()
        });

        let policy = engine.get_effective_policy("ml").await.unwrap();
        assert!((policy.drift_sensitivity - 5.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn get_effective_policy_not_found() {
        let engine = DefaultPolicyEngine::new(1800);
        let result = engine.get_effective_policy("nonexistent").await;
        assert!(matches!(result, Err(PolicyError::PolicyNotFound(_))));
    }
}
