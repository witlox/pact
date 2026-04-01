# Interface: Node Management Backend

**Spec reference:** `specs/features/node-management-delegation.feature`, invariants NM-I1..NM-I5
**Location:** Trait in `pact-common`, implementations in `pact-cli`

---

## Trait: `NodeManagementBackend`

```rust
/// Pluggable backend for BMC/power operations.
/// One implementation per deployment (NM-I1).
///
/// Implementations: CsmBackend, OpenChamiBackend
/// NM-ADV-2: Uses RPITIT (stable since Rust 1.75) instead of async_trait
/// to avoid adding async dependency to pact-common.
pub trait NodeManagementBackend: Send + Sync {
    /// Power cycle a node. (NM-I2: caller must audit before calling)
    fn reboot(&self, node_id: &str) -> impl std::future::Future<Output = Result<String, NodeMgmtError>> + Send;

    /// Reimage a node (reboot with fresh boot from infrastructure).
    /// CSM: BOS session (operation: reboot). OpenCHAMI: Redfish PowerCycle.
    /// Semantics are normalized (NM-I5).
    fn reimage(&self, node_id: &str) -> impl std::future::Future<Output = Result<String, NodeMgmtError>> + Send;

    /// HSM path prefix for this backend.
    /// CSM: "/smd/hsm/v2". OpenCHAMI: "/hsm/v2".
    fn hsm_path_prefix(&self) -> &str;

    /// Backend display name for audit entries and error messages.
    fn backend_name(&self) -> &str;
}
```

---

## Error type: `NodeMgmtError`

```rust
/// Errors from node management delegation.
/// Lives in pact-common (shared by pact-cli and pact-mcp).
#[derive(Debug, thiserror::Error)]
pub enum NodeMgmtError {
    #[error("node management backend not configured")]
    NotConfigured,

    #[error("backend unreachable: {0}")]
    Unreachable(String),

    #[error("backend returned error: HTTP {status} — {body}")]
    BackendError { status: u16, body: String },

    #[error("authentication failed: {0}")]
    AuthError(String),
}
```

---

## Backend type enum

```rust
/// Selected per deployment (NM-I1). Stored in DelegationConfig.
/// No default — must be explicitly configured when node_mgmt_base_url is set.
/// (NM-ADV-4: CSM is the incumbent, OpenCHAMI is future. No safe default.)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeMgmtBackendType {
    /// HPE Cray System Management (CAPMC + BOS + HSM)
    Csm,
    /// OpenCHAMI (SMD Redfish + BSS + HSM)
    Ochami,
}

impl NodeMgmtBackendType {
    /// Display name for audit entries (NM-ADV-5: needed before backend is created).
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Csm => "CSM",
            Self::Ochami => "OpenCHAMI",
        }
    }
}
```

---

## Config changes: `DelegationConfig`

```rust
pub struct DelegationConfig {
    // Existing lattice fields (unchanged)
    pub lattice_endpoint: Option<String>,
    pub lattice_token: Option<String>,

    // Node management (replaces openchami_smd_url / openchami_token)
    // NM-ADV-4: No default — must be set explicitly. None = use legacy fallback.
    pub node_mgmt_backend: Option<NodeMgmtBackendType>,
    pub node_mgmt_base_url: Option<String>,
    pub node_mgmt_token: Option<String>,
    pub timeout_secs: u64,

    // Deprecated — kept for backward compat, mapped to node_mgmt_* at load
    #[serde(default)]
    pub openchami_smd_url: Option<String>,
    #[serde(default)]
    pub openchami_token: Option<String>,
}
```

---

## Env var mapping

| Env var | Maps to | Notes |
|---------|---------|-------|
| `PACT_NODE_MGMT_BACKEND` | `node_mgmt_backend` | "csm" or "ochami" |
| `PACT_NODE_MGMT_URL` | `node_mgmt_base_url` | Base URL for all APIs |
| `PACT_NODE_MGMT_TOKEN` | `node_mgmt_token` | Bearer token (opaque) |
| `PACT_OPENCHAMI_SMD_URL` | Fallback for `node_mgmt_base_url` | Backward compat |
| `PACT_OPENCHAMI_TOKEN` | Fallback for `node_mgmt_token` | Backward compat |
| `PACT_HSM_PATH_PREFIX` | HSM API path prefix | Override for node import. Default: `/hsm/v2` (CSM uses `/smd/hsm/v2`) |

---

## Factory function

```rust
/// Create the appropriate backend from config.
/// Lives in pact-cli (not pact-common — backends need reqwest).
///
/// NM-ADV-1: Backward compat fallback (openchami_smd_url) ONLY applies
/// when backend is Ochami. Prevents silent misconfiguration where CSM
/// backend gets an OpenCHAMI URL.
pub fn create_node_mgmt_backend(
    config: &DelegationConfig,
) -> Result<Box<dyn NodeManagementBackend>, NodeMgmtError> {
    let Some(ref backend_type) = config.node_mgmt_backend else {
        // No backend configured — check legacy env vars for backward compat
        if let Some(ref url) = config.openchami_smd_url {
            let token = config.node_mgmt_token.as_deref()
                .or(config.openchami_token.as_deref());
            return Ok(Box::new(OpenChamiBackend::new(url, token, config.timeout_secs)));
        }
        return Err(NodeMgmtError::NotConfigured);
    };

    let base_url = match backend_type {
        NodeMgmtBackendType::Ochami => config.node_mgmt_base_url.as_deref()
            .or(config.openchami_smd_url.as_deref()),  // backward compat only for ochami
        NodeMgmtBackendType::Csm => config.node_mgmt_base_url.as_deref(),
    }.ok_or(NodeMgmtError::NotConfigured)?;

    let token = config.node_mgmt_token.as_deref()
        .or(config.openchami_token.as_deref());

    match backend_type {
        NodeMgmtBackendType::Csm => Ok(Box::new(CsmBackend::new(base_url, token, config.timeout_secs))),
        NodeMgmtBackendType::Ochami => Ok(Box::new(OpenChamiBackend::new(base_url, token, config.timeout_secs))),
    }
}
```

---

## Implementation: `CsmBackend`

**Location:** `pact-cli/src/commands/csm.rs` (~120 lines)

```rust
pub struct CsmBackend {
    base_url: String,
    token: Option<String>,
    client: reqwest::Client,
}

// reboot → POST {base_url}/capmc/capmc/v1/xname_reinit
//   body: {"reason":"pact reboot","xnames":["<node_id>"],"force":false}
//
// reimage → POST {base_url}/bos/v2/sessions
//   body: {"operation":"reboot","limit":"<node_id>"}
//
// hsm_path_prefix → "/smd/hsm/v2"
// backend_name → "CSM"
```

---

## Implementation: `OpenChamiBackend`

**Location:** `pact-cli/src/commands/openchami.rs` (rename existing, implement trait)

```rust
pub struct OpenChamiBackend { /* existing OpenChamiClient fields */ }

// reboot → POST {base_url}/hsm/v2/State/Components/{node_id}/Actions/PowerCycle
//   body: {"ResetType":"ForceRestart"}
//
// reimage → same as reboot (BSS handles image selection)
//
// hsm_path_prefix → "/hsm/v2"
// backend_name → "OpenCHAMI"
```

---

## Integration with delegate.rs

`reboot_node()` and `reimage_node()` change from directly constructing `OpenChamiClient` to calling `create_node_mgmt_backend()` and using the trait. Audit logging is unchanged (NM-I2) — still happens before the backend call.

NM-ADV-5: The `target_system` in the audit entry uses `config.node_mgmt_backend.display_name()` (from config, not backend instance) since audit precedes backend creation. The `DelegationResult.target_system` uses `backend.backend_name()` for post-call reporting.

---

## Integration with node.rs (HSM import)

`query_smd_components()` and `query_smd_ethernet()` use `backend.hsm_path_prefix()` to construct the correct URL path. The rest of the import logic is unchanged — HSM response format is identical.

---

## Invariant enforcement

| Invariant | Enforcement point | Mechanism |
|-----------|-------------------|-----------|
| NM-I1 (one backend) | `DelegationConfig` | Single `node_mgmt_backend` field, enum not per-node |
| NM-I2 (audit before call) | `delegate.rs:reboot_node/reimage_node` | `audit_delegation()` called before `backend.reboot/reimage()` |
| NM-I3 (graceful failure) | `NodeMgmtError` | All errors are typed, no panics, caller formats for user |
| NM-I4 (single credential) | `DelegationConfig` | One `node_mgmt_base_url` + `node_mgmt_token` for all operations |
| NM-I5 (uniform semantics) | `NodeManagementBackend` trait | `reimage()` signature identical for both backends |
