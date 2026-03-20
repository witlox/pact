//! Role-based access control engine.
//!
//! Role model (from ARCHITECTURE.md):
//! - `pact-platform-admin` — full system access (P6: always authorized)
//! - `pact-ops-{vcluster}` — day-to-day ops scoped to one vCluster
//! - `pact-viewer-{vcluster}` — read-only per-vCluster
//! - `pact-regulated-{vcluster}` — like ops but requires two-person approval
//! - `pact-service-agent` — machine identity for agents
//! - `pact-service-ai` — machine identity for AI agents (P8: no emergency)
//!
//! RBAC is the first authorization layer. Complex rules defer to OPA.

use pact_common::types::{Identity, Scope, VClusterPolicy};

/// RBAC evaluation result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RbacDecision {
    /// Operation is allowed.
    Allow,
    /// Operation is denied with a reason.
    Deny { reason: String },
    /// RBAC cannot determine — defer to OPA for complex rules.
    Defer,
}

/// Actions that can be authorized.
pub mod actions {
    pub const COMMIT: &str = "commit";
    pub const ROLLBACK: &str = "rollback";
    pub const EXEC: &str = "exec";
    pub const SHELL: &str = "shell";
    pub const EMERGENCY_START: &str = "emergency_start";
    pub const EMERGENCY_END: &str = "emergency_end";
    pub const SERVICE_START: &str = "service_start";
    pub const SERVICE_STOP: &str = "service_stop";
    pub const SERVICE_RESTART: &str = "service_restart";
    pub const STATUS: &str = "status";
    pub const DIFF: &str = "diff";
    pub const LOG: &str = "log";
    pub const APPROVE: &str = "approve";
    pub const POLICY_UPDATE: &str = "policy_update";
}

/// Read-only actions that viewers can perform.
const READ_ACTIONS: &[&str] = &[actions::STATUS, actions::DIFF, actions::LOG];

/// State-changing actions that require ops or higher.
const WRITE_ACTIONS: &[&str] = &[
    actions::COMMIT,
    actions::ROLLBACK,
    actions::EXEC,
    actions::SHELL,
    actions::SERVICE_START,
    actions::SERVICE_STOP,
    actions::SERVICE_RESTART,
];

/// RBAC engine — evaluates access control based on role and scope.
pub struct RbacEngine;

impl RbacEngine {
    /// Evaluate whether an identity is authorized for an action on a scope.
    ///
    /// This implements invariants P1-P8.
    #[allow(clippy::too_many_lines)]
    pub fn evaluate(
        identity: &Identity,
        action: &str,
        scope: &Scope,
        policy: &VClusterPolicy,
    ) -> RbacDecision {
        // P6: Platform admin always authorized
        if identity.role == "pact-platform-admin" {
            return RbacDecision::Allow;
        }

        // P8: AI agent cannot enter/exit emergency mode
        if identity.role == "pact-service-ai"
            && (action == actions::EMERGENCY_START || action == actions::EMERGENCY_END)
        {
            return RbacDecision::Deny {
                reason: "AI agents cannot enter or exit emergency mode (P8)".into(),
            };
        }

        // Extract vCluster from scope
        let vcluster_id = match scope {
            Scope::VCluster(vc) | Scope::Node(vc) => vc.as_str(),
            Scope::Global => {
                // Global scope requires platform admin (already handled above)
                return RbacDecision::Deny {
                    reason: "global scope requires pact-platform-admin role".into(),
                };
            }
        };

        // P3: Check role is scoped to the correct vCluster
        let role_vcluster = extract_vcluster_from_role(&identity.role);

        if let Some(role_vc) = role_vcluster {
            if role_vc != vcluster_id {
                return RbacDecision::Deny {
                    reason: format!(
                        "role {} is scoped to vCluster {role_vc}, not {vcluster_id} (P3)",
                        identity.role
                    ),
                };
            }
        }

        // Policy update requires platform admin
        if action == actions::POLICY_UPDATE {
            return RbacDecision::Deny {
                reason: "policy_update requires pact-platform-admin role".into(),
            };
        }

        // Check role type
        if identity.role.starts_with("pact-viewer-") {
            // Viewers: read-only actions only (P2)
            if READ_ACTIONS.contains(&action) {
                return RbacDecision::Allow;
            }
            return RbacDecision::Deny {
                reason: format!("viewer role cannot perform action: {action} (P2)"),
            };
        }

        if identity.role.starts_with("pact-regulated-") {
            // P4: Regulated role requires two-person approval for state-changing actions
            if READ_ACTIONS.contains(&action) {
                return RbacDecision::Allow;
            }
            if policy.two_person_approval && WRITE_ACTIONS.contains(&action) {
                // Don't deny — signal that approval is required
                return RbacDecision::Defer; // Caller creates PendingApproval
            }
            if WRITE_ACTIONS.contains(&action) || action == actions::APPROVE {
                return RbacDecision::Allow;
            }
            return RbacDecision::Deny {
                reason: format!("action not permitted for regulated role: {action}"),
            };
        }

        if identity.role.starts_with("pact-ops-") {
            // Ops: read and write actions
            if READ_ACTIONS.contains(&action) || WRITE_ACTIONS.contains(&action) {
                return RbacDecision::Allow;
            }
            if action == actions::EMERGENCY_START || action == actions::EMERGENCY_END {
                return RbacDecision::Allow;
            }
            if action == actions::APPROVE {
                return RbacDecision::Allow;
            }
            return RbacDecision::Deny {
                reason: format!("action not permitted for ops role: {action}"),
            };
        }

        if identity.role == "pact-service-agent" {
            // Service agent: limited machine operations
            if action == actions::STATUS || action == actions::LOG {
                return RbacDecision::Allow;
            }
            return RbacDecision::Deny {
                reason: format!("service agent cannot perform action: {action}"),
            };
        }

        if identity.role == "pact-service-ai" {
            // AI agent: limited write access (already checked P8 emergency above)
            if READ_ACTIONS.contains(&action) || action == actions::EXEC {
                return RbacDecision::Allow;
            }
            return RbacDecision::Deny {
                reason: format!("AI agent cannot perform action: {action}"),
            };
        }

        // Check role bindings from VClusterPolicy
        for binding in &policy.role_bindings {
            if binding.role == identity.role
                && binding.principals.contains(&identity.principal)
                && binding.allowed_actions.iter().any(|a| a == action || a == "*")
            {
                // F20 fix: wildcard bindings must not grant emergency access.
                // Emergency actions require explicit listing, not wildcards.
                if (action == actions::EMERGENCY_START || action == actions::EMERGENCY_END)
                    && !binding.allowed_actions.iter().any(|a| a == action)
                {
                    return RbacDecision::Deny {
                        reason: "emergency actions require explicit binding, not wildcard".into(),
                    };
                }
                return RbacDecision::Allow;
            }
        }

        // Unknown role — deny
        RbacDecision::Deny { reason: format!("unknown role: {}", identity.role) }
    }
}

/// Extract the vCluster suffix from a role string.
///
/// "pact-ops-ml-training" → Some("ml-training")
/// "pact-viewer-dev" → Some("dev")
/// "pact-platform-admin" → None
/// "pact-service-agent" → None
fn extract_vcluster_from_role(role: &str) -> Option<&str> {
    if let Some(vc) = role.strip_prefix("pact-ops-") {
        return Some(vc);
    }
    if let Some(vc) = role.strip_prefix("pact-viewer-") {
        return Some(vc);
    }
    if let Some(vc) = role.strip_prefix("pact-regulated-") {
        return Some(vc);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use pact_common::types::PrincipalType;

    fn identity(principal: &str, role: &str) -> Identity {
        Identity {
            principal: principal.into(),
            principal_type: PrincipalType::Human,
            role: role.into(),
        }
    }

    fn service_identity(principal: &str, role: &str) -> Identity {
        Identity {
            principal: principal.into(),
            principal_type: PrincipalType::Service,
            role: role.into(),
        }
    }

    fn scope_vc(vc: &str) -> Scope {
        Scope::VCluster(vc.into())
    }

    fn default_policy(vc: &str) -> VClusterPolicy {
        VClusterPolicy { vcluster_id: vc.into(), ..VClusterPolicy::default() }
    }

    fn regulated_policy(vc: &str) -> VClusterPolicy {
        VClusterPolicy {
            vcluster_id: vc.into(),
            two_person_approval: true,
            regulated: true,
            ..VClusterPolicy::default()
        }
    }

    // --- P6: Platform admin always authorized ---

    #[test]
    fn platform_admin_can_do_anything() {
        let admin = identity("admin@example.com", "pact-platform-admin");
        let policy = default_policy("ml");

        assert_eq!(
            RbacEngine::evaluate(&admin, actions::COMMIT, &scope_vc("ml"), &policy),
            RbacDecision::Allow
        );
        assert_eq!(
            RbacEngine::evaluate(&admin, actions::EXEC, &scope_vc("ml"), &policy),
            RbacDecision::Allow
        );
        assert_eq!(
            RbacEngine::evaluate(&admin, actions::SHELL, &scope_vc("ml"), &policy),
            RbacDecision::Allow
        );
        assert_eq!(
            RbacEngine::evaluate(&admin, actions::EMERGENCY_START, &scope_vc("ml"), &policy),
            RbacDecision::Allow
        );
        assert_eq!(
            RbacEngine::evaluate(&admin, actions::POLICY_UPDATE, &scope_vc("ml"), &policy),
            RbacDecision::Allow
        );
    }

    #[test]
    fn platform_admin_can_access_any_vcluster() {
        let admin = identity("admin@example.com", "pact-platform-admin");
        let policy = default_policy("other-vc");

        assert_eq!(
            RbacEngine::evaluate(&admin, actions::COMMIT, &scope_vc("other-vc"), &policy),
            RbacDecision::Allow
        );
    }

    #[test]
    fn platform_admin_can_access_global_scope() {
        let admin = identity("admin@example.com", "pact-platform-admin");
        let policy = default_policy("ml");

        assert_eq!(
            RbacEngine::evaluate(&admin, actions::STATUS, &Scope::Global, &policy),
            RbacDecision::Allow
        );
    }

    // --- P3: Role scoping ---

    #[test]
    fn ops_role_scoped_to_vcluster() {
        let ops = identity("ops@example.com", "pact-ops-ml");
        let policy = default_policy("ml");

        assert_eq!(
            RbacEngine::evaluate(&ops, actions::COMMIT, &scope_vc("ml"), &policy),
            RbacDecision::Allow
        );
    }

    #[test]
    fn ops_role_denied_on_wrong_vcluster() {
        let ops = identity("ops@example.com", "pact-ops-ml");
        let policy = default_policy("other");

        let result = RbacEngine::evaluate(&ops, actions::COMMIT, &scope_vc("other"), &policy);
        assert!(matches!(result, RbacDecision::Deny { .. }));
    }

    #[test]
    fn ops_role_denied_on_global_scope() {
        let ops = identity("ops@example.com", "pact-ops-ml");
        let policy = default_policy("ml");

        let result = RbacEngine::evaluate(&ops, actions::COMMIT, &Scope::Global, &policy);
        assert!(matches!(result, RbacDecision::Deny { .. }));
    }

    // --- P2: Viewer role ---

    #[test]
    fn viewer_can_read() {
        let viewer = identity("viewer@example.com", "pact-viewer-ml");
        let policy = default_policy("ml");

        assert_eq!(
            RbacEngine::evaluate(&viewer, actions::STATUS, &scope_vc("ml"), &policy),
            RbacDecision::Allow
        );
        assert_eq!(
            RbacEngine::evaluate(&viewer, actions::DIFF, &scope_vc("ml"), &policy),
            RbacDecision::Allow
        );
        assert_eq!(
            RbacEngine::evaluate(&viewer, actions::LOG, &scope_vc("ml"), &policy),
            RbacDecision::Allow
        );
    }

    #[test]
    fn viewer_cannot_write() {
        let viewer = identity("viewer@example.com", "pact-viewer-ml");
        let policy = default_policy("ml");

        let result = RbacEngine::evaluate(&viewer, actions::COMMIT, &scope_vc("ml"), &policy);
        assert!(matches!(result, RbacDecision::Deny { .. }));

        let result = RbacEngine::evaluate(&viewer, actions::EXEC, &scope_vc("ml"), &policy);
        assert!(matches!(result, RbacDecision::Deny { .. }));

        let result = RbacEngine::evaluate(&viewer, actions::SHELL, &scope_vc("ml"), &policy);
        assert!(matches!(result, RbacDecision::Deny { .. }));
    }

    #[test]
    fn viewer_denied_on_wrong_vcluster() {
        let viewer = identity("viewer@example.com", "pact-viewer-ml");
        let policy = default_policy("other");

        let result = RbacEngine::evaluate(&viewer, actions::STATUS, &scope_vc("other"), &policy);
        assert!(matches!(result, RbacDecision::Deny { .. }));
    }

    // --- Ops actions ---

    #[test]
    fn ops_can_do_write_actions() {
        let ops = identity("ops@example.com", "pact-ops-ml");
        let policy = default_policy("ml");

        for action in &[
            actions::COMMIT,
            actions::EXEC,
            actions::SHELL,
            actions::ROLLBACK,
            actions::SERVICE_START,
            actions::SERVICE_STOP,
            actions::SERVICE_RESTART,
        ] {
            assert_eq!(
                RbacEngine::evaluate(&ops, action, &scope_vc("ml"), &policy),
                RbacDecision::Allow,
                "ops should be allowed to {action}"
            );
        }
    }

    #[test]
    fn ops_can_emergency() {
        let ops = identity("ops@example.com", "pact-ops-ml");
        let policy = default_policy("ml");

        assert_eq!(
            RbacEngine::evaluate(&ops, actions::EMERGENCY_START, &scope_vc("ml"), &policy),
            RbacDecision::Allow
        );
        assert_eq!(
            RbacEngine::evaluate(&ops, actions::EMERGENCY_END, &scope_vc("ml"), &policy),
            RbacDecision::Allow
        );
    }

    #[test]
    fn ops_cannot_update_policy() {
        let ops = identity("ops@example.com", "pact-ops-ml");
        let policy = default_policy("ml");

        let result = RbacEngine::evaluate(&ops, actions::POLICY_UPDATE, &scope_vc("ml"), &policy);
        assert!(matches!(result, RbacDecision::Deny { .. }));
    }

    // --- P4: Regulated role + two-person approval ---

    #[test]
    fn regulated_role_defers_on_write_with_two_person_approval() {
        let regulated = identity("ops@example.com", "pact-regulated-ml");
        let policy = regulated_policy("ml");

        let result = RbacEngine::evaluate(&regulated, actions::COMMIT, &scope_vc("ml"), &policy);
        assert_eq!(
            result,
            RbacDecision::Defer,
            "regulated + two_person_approval should defer to approval workflow"
        );
    }

    #[test]
    fn regulated_role_allows_read() {
        let regulated = identity("ops@example.com", "pact-regulated-ml");
        let policy = regulated_policy("ml");

        assert_eq!(
            RbacEngine::evaluate(&regulated, actions::STATUS, &scope_vc("ml"), &policy),
            RbacDecision::Allow
        );
    }

    #[test]
    fn regulated_role_allows_write_without_two_person_policy() {
        let regulated = identity("ops@example.com", "pact-regulated-ml");
        let policy = default_policy("ml"); // two_person_approval = false

        assert_eq!(
            RbacEngine::evaluate(&regulated, actions::COMMIT, &scope_vc("ml"), &policy),
            RbacDecision::Allow,
            "regulated role should allow writes when two_person_approval is not required"
        );
    }

    // --- P8: AI agent emergency restriction ---

    #[test]
    fn ai_agent_cannot_emergency() {
        let ai = service_identity("claude-agent", "pact-service-ai");
        let policy = default_policy("ml");

        let result = RbacEngine::evaluate(&ai, actions::EMERGENCY_START, &scope_vc("ml"), &policy);
        assert!(matches!(result, RbacDecision::Deny { reason } if reason.contains("P8")));

        let result = RbacEngine::evaluate(&ai, actions::EMERGENCY_END, &scope_vc("ml"), &policy);
        assert!(matches!(result, RbacDecision::Deny { reason } if reason.contains("P8")));
    }

    #[test]
    fn ai_agent_can_exec() {
        let ai = service_identity("claude-agent", "pact-service-ai");
        let policy = default_policy("ml");

        assert_eq!(
            RbacEngine::evaluate(&ai, actions::EXEC, &scope_vc("ml"), &policy),
            RbacDecision::Allow
        );
    }

    #[test]
    fn ai_agent_can_read() {
        let ai = service_identity("claude-agent", "pact-service-ai");
        let policy = default_policy("ml");

        assert_eq!(
            RbacEngine::evaluate(&ai, actions::STATUS, &scope_vc("ml"), &policy),
            RbacDecision::Allow
        );
    }

    #[test]
    fn ai_agent_cannot_shell() {
        let ai = service_identity("claude-agent", "pact-service-ai");
        let policy = default_policy("ml");

        let result = RbacEngine::evaluate(&ai, actions::SHELL, &scope_vc("ml"), &policy);
        assert!(matches!(result, RbacDecision::Deny { .. }));
    }

    // --- Service agent ---

    #[test]
    fn service_agent_limited_access() {
        let agent = service_identity("pact-agent-node-001", "pact-service-agent");
        let policy = default_policy("ml");

        assert_eq!(
            RbacEngine::evaluate(&agent, actions::STATUS, &scope_vc("ml"), &policy),
            RbacDecision::Allow
        );
        assert_eq!(
            RbacEngine::evaluate(&agent, actions::LOG, &scope_vc("ml"), &policy),
            RbacDecision::Allow
        );

        let result = RbacEngine::evaluate(&agent, actions::EXEC, &scope_vc("ml"), &policy);
        assert!(matches!(result, RbacDecision::Deny { .. }));
    }

    // --- Role binding from policy ---

    #[test]
    fn custom_role_binding_allows_action() {
        let user = identity("custom@example.com", "custom-role");
        let mut policy = default_policy("ml");
        policy.role_bindings.push(pact_common::types::RoleBinding {
            role: "custom-role".into(),
            principals: vec!["custom@example.com".into()],
            allowed_actions: vec!["commit".into(), "exec".into()],
        });

        assert_eq!(
            RbacEngine::evaluate(&user, actions::COMMIT, &scope_vc("ml"), &policy),
            RbacDecision::Allow
        );
    }

    #[test]
    fn custom_role_binding_denies_unlisted_action() {
        let user = identity("custom@example.com", "custom-role");
        let mut policy = default_policy("ml");
        policy.role_bindings.push(pact_common::types::RoleBinding {
            role: "custom-role".into(),
            principals: vec!["custom@example.com".into()],
            allowed_actions: vec!["status".into()],
        });

        let result = RbacEngine::evaluate(&user, actions::COMMIT, &scope_vc("ml"), &policy);
        assert!(matches!(result, RbacDecision::Deny { .. }));
    }

    #[test]
    fn wildcard_binding_allows_non_emergency_actions() {
        let user = identity("superuser@example.com", "custom-super");
        let mut policy = default_policy("ml");
        policy.role_bindings.push(pact_common::types::RoleBinding {
            role: "custom-super".into(),
            principals: vec!["superuser@example.com".into()],
            allowed_actions: vec!["*".into()],
        });

        assert_eq!(
            RbacEngine::evaluate(&user, actions::COMMIT, &scope_vc("ml"), &policy),
            RbacDecision::Allow
        );
        // F20 fix: wildcard must NOT grant emergency access
        let result =
            RbacEngine::evaluate(&user, actions::EMERGENCY_START, &scope_vc("ml"), &policy);
        assert!(
            matches!(result, RbacDecision::Deny { .. }),
            "wildcard binding should not grant emergency access"
        );
    }

    #[test]
    fn explicit_emergency_binding_allows_emergency() {
        let user = identity("oncall@example.com", "custom-oncall");
        let mut policy = default_policy("ml");
        policy.role_bindings.push(pact_common::types::RoleBinding {
            role: "custom-oncall".into(),
            principals: vec!["oncall@example.com".into()],
            allowed_actions: vec!["emergency_start".into(), "emergency_end".into()],
        });

        assert_eq!(
            RbacEngine::evaluate(&user, actions::EMERGENCY_START, &scope_vc("ml"), &policy),
            RbacDecision::Allow
        );
    }

    // --- extract_vcluster_from_role ---

    #[test]
    fn extract_vcluster_from_role_ops() {
        assert_eq!(extract_vcluster_from_role("pact-ops-ml-training"), Some("ml-training"));
    }

    #[test]
    fn extract_vcluster_from_role_viewer() {
        assert_eq!(extract_vcluster_from_role("pact-viewer-dev"), Some("dev"));
    }

    #[test]
    fn extract_vcluster_from_role_platform_admin() {
        assert_eq!(extract_vcluster_from_role("pact-platform-admin"), None);
    }

    #[test]
    fn extract_vcluster_from_role_service() {
        assert_eq!(extract_vcluster_from_role("pact-service-agent"), None);
    }

    // --- Unknown role ---

    #[test]
    fn unknown_role_denied() {
        let user = identity("unknown@example.com", "random-role");
        let policy = default_policy("ml");

        let result = RbacEngine::evaluate(&user, actions::STATUS, &scope_vc("ml"), &policy);
        assert!(matches!(result, RbacDecision::Deny { .. }));
    }
}
