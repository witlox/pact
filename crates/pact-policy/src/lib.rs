//! `pact-policy` — IAM and policy evaluation engine.
//!
//! Library crate linked into `pact-journal` (ADR-003). Provides:
//! - **iam**: OIDC token validation, JWKS caching, identity extraction
//! - **rbac**: Role-based access control with vCluster scoping (P1-P8)
//! - **rules**: Policy evaluation engine with two-person approval workflow
//! - **federation**: Sovra policy template synchronization (feature-gated)
//!
//! Policy evaluation flow:
//! 1. Authenticate (OIDC token → Identity)
//! 2. RBAC check (fast, local) → Allow/Deny/Defer
//! 3. If Defer + two_person_approval: create PendingApproval
//! 4. If Defer + OPA available: evaluate Rego rules (feature-gated)
//! 5. Degraded mode (P7): cached VClusterPolicy only
//!
//! See `docs/decisions/ADR-003-policy-engine.md` for design rationale.

pub mod federation;
pub mod iam;
pub mod rbac;
pub mod rules;
