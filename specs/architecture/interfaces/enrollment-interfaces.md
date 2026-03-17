# Enrollment Interfaces

Interfaces for node enrollment, domain membership, certificate lifecycle, and vCluster assignment. Source: ADR-008, node_enrollment.feature, invariants E1-E10.

---

## EnrollmentService (gRPC — new proto: enrollment.proto)

Journal-hosted gRPC service. Handles agent boot enrollment (unauthenticated) and admin node management (authenticated).

```rust
/// Source: ADR-008, node_enrollment.feature
/// Hosted on: pact-journal (same server as ConfigService, PolicyService)
///
/// IMPORTANT: Enroll is the ONLY unauthenticated gRPC method in the system (E1).
/// All other methods require mTLS + OIDC Bearer token.
/// Enrollment endpoint is rate-limited (default 100/minute) and audit-logged.
service EnrollmentService {
    /// Agent boot enrollment — server-TLS-only (no client cert).
    /// Agent presents hardware identity + CSR, journal matches against
    /// enrollment registry, signs CSR with intermediate CA if match found.
    /// Source: E1, E7, node_enrollment.feature "Agent enrolls on first boot"
    rpc Enroll(EnrollRequest) -> EnrollResponse;

    /// Agent certificate renewal — mTLS authenticated.
    /// Agent presents current cert serial + new CSR, journal signs locally.
    /// Source: E5, E6, node_enrollment.feature "Agent requests certificate renewal"
    rpc RenewCert(RenewCertRequest) -> RenewCertResponse;

    /// Admin: register a node in the enrollment registry.
    /// Source: E10, node_enrollment.feature "Platform admin enrolls a node"
    rpc RegisterNode(RegisterNodeRequest) -> RegisterNodeResponse;

    /// Admin: batch register nodes.
    /// Not atomic — each node is an independent Raft command. Returns per-node status.
    /// Source: node_enrollment.feature "Batch enrollment of multiple nodes"
    rpc BatchRegisterNodes(BatchRegisterRequest) -> BatchRegisterResponse;

    /// Admin: decommission a node (revoke cert, remove from registry).
    /// Warns if active sessions exist; requires --force to proceed.
    /// Source: E9, E10, node_enrollment.feature "Decommission a node"
    rpc DecommissionNode(DecommissionRequest) -> DecommissionResponse;

    /// Admin: assign node to a vCluster.
    /// Source: E8, node_enrollment.feature "Assign enrolled node to a vCluster"
    rpc AssignNode(AssignNodeRequest) -> AssignNodeResponse;

    /// Admin: unassign node from vCluster (maintenance mode).
    /// Source: E8, node_enrollment.feature "Unassign node from vCluster"
    rpc UnassignNode(UnassignNodeRequest) -> UnassignNodeResponse;

    /// Admin: move node between vClusters (atomic unassign + assign).
    /// Source: node_enrollment.feature "Move node between vClusters"
    rpc MoveNode(MoveNodeRequest) -> MoveNodeResponse;

    /// Query: list nodes with optional filters.
    /// Source: node_enrollment.feature "List enrolled nodes"
    rpc ListNodes(ListNodesRequest) -> stream NodeInfo;

    /// Query: inspect a single node's enrollment details.
    /// Source: node_enrollment.feature "Inspect node details"
    rpc InspectNode(InspectNodeRequest) -> NodeInfo;
}
```

### Request/Response Types

```rust
message EnrollRequest {
    HardwareIdentity hardware_identity = 1;
    bytes csr_pem = 2;  // PEM-encoded PKCS#10 Certificate Signing Request
}

message EnrollResponse {
    string signed_cert_pem = 1;       // Signed by journal's intermediate CA
    string cert_serial = 2;
    Timestamp not_after = 3;
    optional string vcluster_id = 4;  // Current assignment (None = maintenance mode)
    string node_id = 5;               // Resolved node_id from enrollment record
}

message RenewCertRequest {
    string node_id = 1;
    string current_cert_serial = 2;
    bytes new_csr_pem = 3;  // New CSR with new keypair
}

message RenewCertResponse {
    string signed_cert_pem = 1;
    string cert_serial = 2;
    Timestamp not_after = 3;
}

message DecommissionRequest {
    string node_id = 1;
    bool force = 2;  // Required if active sessions exist
}

message DecommissionResponse {
    bool had_active_sessions = 1;
    uint32 sessions_terminated = 2;
}

message BatchRegisterResponse {
    repeated NodeRegistrationResult results = 1;
}

message NodeRegistrationResult {
    string node_id = 1;
    bool success = 2;
    string error = 3;  // Empty on success
}
```

### Contracts

- `Enroll`: Server-TLS-only (no client cert). MUST reject if no matching enrollment record (E1). MUST reject if state is Active (`ALREADY_ACTIVE`) — prevents concurrent enrollment race. MUST reject if state is Revoked (E7). MUST sign CSR with intermediate CA key. MUST return vCluster assignment in response. MUST set state to Active via Raft write. Rate-limited (default 100/minute). All attempts audit-logged.
- `RenewCert`: MUST require mTLS. MUST validate caller's mTLS CN matches `node_id`. MUST validate `current_cert_serial` matches stored serial. MUST sign new CSR with intermediate CA. Local CPU operation — no external calls.
- `RegisterNode`: MUST require `pact-platform-admin` role (E10). MUST reject duplicate node_id or duplicate hardware identity within the domain (E2).
- `BatchRegisterNodes`: Same RBAC as RegisterNode. NOT atomic — each node is independent. Returns per-node success/failure.
- `DecommissionNode`: MUST require `pact-platform-admin` role (E10). If active sessions exist and `force=false`, return warning with session count. On proceed: set state to Revoked, add cert serial to Raft revocation registry (E9), terminate agent mTLS connection.
- `AssignNode`: MUST require `pact-platform-admin` or `pact-ops-{target_vcluster}` role (E10). Node MUST be enrolled (any state except Revoked).
- `UnassignNode`: Same RBAC as AssignNode. Sets vCluster to None.
- `MoveNode`: Same RBAC as AssignNode for both source and target vCluster. Atomic via single Raft command.
- `ListNodes`: RBAC-filtered. `pact-platform-admin` sees all. `pact-ops-{vc}` / `pact-viewer-{vc}` sees only nodes in their vCluster. Unassigned nodes visible only to platform-admin.
- `InspectNode`: Same RBAC filtering as ListNodes.

---

## CaKeyManager (internal — pact-journal)

Interface for the journal's intermediate CA key. Each journal node generates an ephemeral
intermediate CA key at startup. NOT stored in Raft — exists in memory only.

```rust
/// Source: ADR-008
/// Each journal node manages its own copy of the intermediate CA key.
pub trait CaKeyManager: Send + Sync {
    /// Sign a CSR using the intermediate CA key.
    /// CPU-only operation (~1ms). No network call.
    /// Source: ADR-008 "CSR model", E4
    fn sign_csr(
        &self,
        csr_pem: &[u8],
        node_id: &str,
        domain_id: &str,
        ttl: Duration,
    ) -> Result<SignedCert, CaError>;

    /// Get the current CA certificate (for inclusion in cert chain).
    fn ca_cert(&self) -> &str;

    /// Check if the CA key needs rotation (approaching expiry).
    fn needs_rotation(&self) -> bool;
}
```

### Contracts

- `sign_csr`: cert CN = `pact-service-agent/{node_id}@{domain_id}`. TTL from domain config (default 3 days). Signs locally — MUST NOT make network calls.
- CA key is generated ephemeral at journal startup (in memory only, never persisted to disk).
- CA key rotation happens automatically on journal restart. The CA certificate is distributed to agents via the enrollment response chain.

---

## RevocationRegistry (internal — pact-journal)

Interface for certificate revocation. Used only on node decommission. Revoked cert
serials are stored in the Raft revocation registry and replicated to all journal nodes.

```rust
/// Source: ADR-008, E9
/// Used only for certificate revocation on decommission.
/// Revocation state is stored in Raft — no external dependency.
pub trait RevocationRegistry: Send + Sync {
    /// Add a revoked cert serial to the Raft revocation registry.
    /// Called on node decommission (E9).
    async fn revoke_cert(&self, serial: &str) -> Result<(), RevocationError>;

    /// Check if a cert serial is revoked.
    fn is_revoked(&self, serial: &str) -> bool;

    /// List all revoked cert serials.
    fn revoked_serials(&self) -> Vec<String>;
}
```

### Contracts

- `revoke_cert`: writes revocation entry to Raft state. Replicated to all journal nodes via consensus. The node is already Revoked in enrollment state — the revocation registry entry ensures mTLS connections using the revoked cert are rejected.
- Journal nodes check incoming mTLS client cert serials against the revocation registry on every connection.

---

## DualChannelClient (pact-agent)

gRPC client with hot-swap certificate rotation. Replaces the existing single-channel `JournalClient`.

```rust
/// Source: ADR-008, E6
/// Wraps all journal gRPC clients with dual-channel rotation.
pub struct DualChannelClient {
    active: Arc<RwLock<ChannelState>>,
    passive: Arc<RwLock<Option<ChannelState>>>,
    renewal_config: RenewalConfig,
}

struct ChannelState {
    channel: Channel,
    config: ConfigServiceClient<Channel>,
    boot: BootConfigServiceClient<Channel>,
    policy: PolicyServiceClient<Channel>,
    enrollment: EnrollmentServiceClient<Channel>,
    cert_serial: String,
    cert_not_after: DateTime<Utc>,
    private_key: Vec<u8>,  // In-memory only, never persisted
}

pub struct RenewalConfig {
    pub renewal_fraction: f64,    // default 0.667 (2/3 of lifetime)
    pub retry_interval: Duration, // default 5 minutes
}

impl DualChannelClient {
    /// Perform certificate renewal and channel rotation.
    /// 1. Generate new keypair + CSR
    /// 2. Call RenewCert over active channel
    /// 3. Build passive channel with new key + signed cert
    /// 4. Health-check passive channel
    /// 5. Atomic swap: passive → active, old active → drain
    ///
    /// Contracts:
    /// - Active channel continues serving during build + swap.
    /// - In-flight RPCs on old channel complete before close.
    /// - On failure at any step: active channel continues, retry later.
    pub async fn rotate(&self) -> Result<(), RotationError>;

    /// Check if renewal is needed (current time > renewal_fraction * lifetime).
    pub fn needs_renewal(&self) -> bool;
}
```

---

## EnrollmentClient (pact-agent)

Agent-side enrollment for boot and renewal.

```rust
/// Source: ADR-008, E1
/// Used at boot (before mTLS is established) and for renewal.
pub trait EnrollmentClient: Send + Sync {
    /// Boot enrollment: present hardware identity + CSR, receive signed cert.
    /// Called over server-TLS-only channel (no client cert yet).
    async fn enroll(
        &self,
        hardware_identity: &HardwareIdentity,
        csr_pem: &[u8],
    ) -> Result<EnrollResponse, EnrollmentError>;

    /// Certificate renewal: present current serial + new CSR, receive new signed cert.
    /// Called over existing mTLS channel.
    async fn renew_cert(
        &self,
        node_id: &str,
        current_serial: &str,
        new_csr_pem: &[u8],
    ) -> Result<RenewCertResponse, EnrollmentError>;
}
```

---

## HardwareIdentity (pact-common)

Value object for node hardware attestation. Read from SMBIOS/DMI at boot.

```rust
/// Source: ADR-008, domain-model.md Node Management context
pub struct HardwareIdentity {
    pub mac_addresses: Vec<String>,       // Primary NIC MAC(s)
    pub bmc_serial: String,               // SMBIOS/DMI BMC serial
    pub tpm_ek_hash: Option<String>,      // TPM endorsement key hash (optional)
}
```

### Detection Contract

- `mac_addresses`: read from `/sys/class/net/*/address` (Linux). Filter loopback and virtual interfaces.
- `bmc_serial`: read from SMBIOS via `/sys/class/dmi/id/board_serial`.
- `tpm_ek_hash`: read from TPM 2.0 device if available. SHA-256 of endorsement public key.
- On non-Linux (macOS dev): return mock/empty values.

---

## CLI Node Commands (pact-cli)

New `pact node` subcommand group. Source: node_enrollment.feature inventory scenarios.

```rust
/// Source: ADR-008, node_enrollment.feature CLI scenarios
enum NodeCommand {
    Enroll {
        node_id: String,
        #[arg(long)]
        mac: String,
        #[arg(long)]
        bmc_serial: Option<String>,
    },
    EnrollBatch {
        #[arg(long)]
        file: PathBuf,
    },
    Decommission {
        node_id: String,
        #[arg(long)]
        force: bool,
    },
    Assign {
        node_id: String,
        #[arg(long)]
        vcluster: String,
    },
    Unassign { node_id: String },
    Move {
        node_id: String,
        #[arg(long)]
        to_vcluster: String,
    },
    List {
        #[arg(long)]
        state: Option<EnrollmentState>,
        #[arg(long)]
        vcluster: Option<String>,
        #[arg(long)]
        unassigned: bool,
    },
    Inspect { node_id: String },
}
```
