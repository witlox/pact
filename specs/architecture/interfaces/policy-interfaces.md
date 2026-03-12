# Policy Interfaces (pact-policy library)

Trait interfaces for the policy engine library crate. Called by pact-journal's PolicyService handler.

---

## PolicyEngine Trait

```rust
/// Main entry point for policy evaluation.
/// Used by: PolicyService gRPC handler in pact-journal.
/// Source: policy_evaluation.feature, invariants P1-P8
#[async_trait]
pub trait PolicyEngine: Send + Sync {
    /// Evaluate an authorization request.
    /// Checks: OIDC token validity → RBAC role/scope → OPA complex rules.
    /// Returns: allow/deny/require-approval with reason.
    async fn evaluate(&self, request: &PolicyRequest) -> Result<PolicyDecision, PactError>;

    /// Get effective policy for a vCluster.
    /// Merges: stored policy + federation overrides (if federation enabled).
    async fn get_effective_policy(&self, vcluster_id: &VClusterId) -> Result<VClusterPolicy, PactError>;
}

pub struct PolicyRequest {
    pub identity: Identity,
    pub scope: Scope,
    pub action: String,               // "commit", "exec", "shell", "emergency", "service"
    pub proposed_change: Option<StateDelta>,
    pub command: Option<String>,       // for exec/shell authorization
}

pub enum PolicyDecision {
    Allow { policy_ref: String },
    Deny { policy_ref: String, reason: String },
    RequireApproval { policy_ref: String, approval_id: String },
}
```

**Contract:**
- Platform admin (P6): always Allow, still logged
- Viewer role: Allow for read ops, Deny for write ops (policy_evaluation.feature: scenarios 4-5)
- Regulated vCluster with two_person_approval (P4): RequireApproval for state-changing ops
- Same admin cannot approve their own request (policy_evaluation.feature: scenario 10)
- AI agent (P8): Deny for emergency mode

## TokenValidator Trait

```rust
/// OIDC token validation.
/// Source: rbac_authorization.feature scenarios 7-9
#[async_trait]
pub trait TokenValidator: Send + Sync {
    /// Validate JWT token: signature, expiry, audience, issuer.
    /// Returns extracted Identity on success.
    async fn validate(&self, token: &str) -> Result<Identity, PactError>;
}
```

**Contract:**
- Expired tokens rejected (scenario 8)
- Wrong audience rejected (scenario 9)
- JWKS cached and rotated

## RbacEngine Trait

```rust
/// Role-based access control evaluation.
/// Source: rbac_authorization.feature, OIDC role model from ARCHITECTURE.md
pub trait RbacEngine: Send + Sync {
    /// Check if identity has permission for action in scope.
    /// Uses role_bindings from VClusterPolicy.
    fn evaluate(&self, identity: &Identity, action: &str, scope: &Scope,
                policy: &VClusterPolicy) -> RbacDecision;
}

pub enum RbacDecision {
    Allow,
    Deny { reason: String },
    Defer,  // Escalate to OPA for complex rules
}
```

## OpaClient Trait

```rust
/// OPA sidecar integration (feature-gated: "opa").
/// Source: ADR-003, interactions.md I7
#[async_trait]
pub trait OpaClient: Send + Sync {
    /// Evaluate Rego policy via REST call to localhost:8181.
    /// Timeout: 1s (not on hot path).
    async fn evaluate(&self, input: &OpaInput) -> Result<OpaResult, PactError>;
    /// Check if OPA sidecar is healthy.
    async fn health(&self) -> bool;
}

pub struct OpaInput {
    pub principal: String,
    pub role: String,
    pub action: String,
    pub scope: String,
    pub context: serde_json::Value,
}

pub struct OpaResult {
    pub allow: bool,
    pub reason: Option<String>,
}
```

**Contract:**
- OPA on localhost only — 1ms latency acceptable (ADR-003)
- OPA unreachable → fall back to cached policy (F7, P7)
- Rego templates loaded from local filesystem or Sovra sync

## PolicyCache

```rust
/// Cached policy for agent degraded mode operation.
/// Source: failure-modes.md F2, invariant P7
pub struct PolicyCache {
    pub policies: HashMap<VClusterId, VClusterPolicy>,
    pub last_refreshed: DateTime<Utc>,
}

impl PolicyCache {
    /// Evaluate using cached policy only (degraded mode).
    /// Whitelist checks: honored.
    /// Two-person approval: denied (fail-closed).
    /// Complex OPA rules: denied (fail-closed).
    /// Platform admin: authorized (logged).
    pub fn evaluate_degraded(&self, request: &PolicyRequest) -> PolicyDecision;
}
```

## FederationSync (feature-gated: "federation")

```rust
/// Sovra federation template sync.
/// Source: federation.feature, interactions.md E5
#[async_trait]
pub trait FederationSync: Send + Sync {
    /// Pull Rego templates from Sovra.
    /// Interval: configurable (default 300s).
    /// Failure: graceful, use cached templates (F10).
    async fn sync(&self) -> Result<(), PactError>;
}
```
