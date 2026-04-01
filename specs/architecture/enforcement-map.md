# Enforcement Map

Maps every invariant to its enforcement point in the codebase — where validation happens, what rejects violations, and how violations are detected.

---

## Journal Invariants

| ID | Invariant | Enforcement Point | Mechanism | Violation Response |
|----|-----------|-------------------|-----------|-------------------|
| J1 | Monotonic sequence, no gaps | `JournalState::apply(AppendEntry)` | `next_sequence` incremented atomically in state machine | Cannot violate — Raft serializes all writes |
| J2 | Immutability after commit | `JournalState` API | No `update_entry` or `delete_entry` method exists. BTreeMap is append-only via `apply()` | Structurally impossible |
| J3 | Authenticated authorship | `JournalState::apply(AppendEntry)` | Validates `author.principal` and `author.role` non-empty | `JournalResponse::ValidationError` |
| J4 | Acyclic parent chain | `JournalState::apply(AppendEntry)` | Validates `parent.is_none() \|\| parent < next_sequence` | `JournalResponse::ValidationError` |
| J5 | Overlay checksum | `JournalState::apply(SetOverlay)` | Validates `checksum == hash(data)` | `JournalResponse::ValidationError` |
| J6 | Single policy per vCluster | `JournalState::apply(SetPolicy)` | HashMap insert replaces existing entry | Structural — HashMap<VClusterId, _> allows only one |
| J7 | Raft consensus for writes | `JournalServer` gRPC handlers | All write RPCs call `raft.client_write()`. No direct state mutation path. | Structural — no bypass exists |
| J8 | Reads from local state | `JournalServer` gRPC handlers | Read RPCs access `JournalState` directly, not via Raft | By design — read methods don't call raft |
| J9 | No duplicate concurrent commits | Raft log serialization | Raft guarantees exactly-once linearizable application | Raft protocol guarantee |

---

## Agent Invariants

| ID | Invariant | Enforcement Point | Mechanism | Violation Response |
|----|-----------|-------------------|-----------|-------------------|
| A1 | At most one commit window | `CommitWindowManager::open()` | `active_window: Option<CommitWindow>` — if Some, extend instead of opening new | Extend existing window |
| A2 | At most one emergency session | `EmergencySession` manager | `active_session: Option<EmergencySession>` — reject if Some | `PactError::EmergencyActive` |
| A3 | Commit window formula | `CommitWindow::compute_duration()` | `window = base / (1 + magnitude * sensitivity)` — always > 0 since all inputs non-negative | Math guarantee (denominator always >= 1) |
| A4 | Auto-rollback on expiry | `CommitWindowManager::tick()` | Timer checks `is_expired(now)`. If expired AND not emergency → rollback | Automatic rollback + journal entry |
| A5 | Active consumer check | `CommitWindowManager::rollback()` | Checks active consumers before reverting | Rollback blocked, alert admin |
| A6 | Service dependency ordering | `ServiceManager::start()` | Services sorted by `order` field. Shutdown in reverse. | Start failure if dependency not ready |
| A7 | Resource budget | Runtime monitoring | Agent RSS < 50MB, CPU < 0.5% steady / < 2% during drift | Operational monitoring (Grafana alert) |
| A8 | Boot time target | Boot sequence timing | Overlay streaming + apply pipeline optimized for < 2s | Operational monitoring |
| A9 | Cached config during partition | `ConfigCache` | On journal connection loss, agent operates from cache. Pending entries queued for replay. | Degraded mode with cached data |
| A10 | Emergency doesn't expand whitelist | `EmergencySession` + `ShellService` | Emergency mode flag only affects commit window expiry, not whitelist evaluation | Whitelist check ignores emergency state |

---

## Drift Invariants

| ID | Invariant | Enforcement Point | Mechanism | Violation Response |
|----|-----------|-------------------|-----------|-------------------|
| D1 | Blacklist exclusion | `DriftEvaluator::evaluate()` | Pattern match against `BlacklistConfig` before processing | Event silently dropped |
| D2 | Seven dimensions | `DriftDimension` enum | Enum has exactly 7 variants: Mounts, Files, Network, Services, Kernel, Packages, Gpu | Compile-time — no other variant possible |
| D3 | Non-negative magnitudes | `DriftVector` + `DriftEvaluator::magnitude()` | All dimension values >= 0 (derived from event counts/deltas). Euclidean norm is non-negative. | Math guarantee |
| D4 | Weight influence | `DriftEvaluator::magnitude()` | `DriftWeights` applied in magnitude calculation. Zero weight = dimension ignored. | Configuration — validated at load time |
| D5 | Observe-only mode | `CommitWindowManager` | Checks `VClusterPolicy.enforcement_mode`. If "observe", logs drift but does not call `open()` | Drift logged to journal without window |

---

## Policy Invariants

| ID | Invariant | Enforcement Point | Mechanism | Violation Response |
|----|-----------|-------------------|-----------|-------------------|
| P1 | Every operation authenticated | gRPC interceptor (all services) | Extract Bearer token from metadata. Call `TokenValidator::validate()`. Reject if missing/invalid. | `tonic::Status::UNAUTHENTICATED` |
| P2 | Every operation authorized | `PolicyEngine::evaluate()` | Called after authentication. Checks RBAC + OPA. | `PolicyDecision::Deny` → `PERMISSION_DENIED` |
| P3 | Role scoping | `RbacEngine::evaluate()` | Role name contains vCluster ID. RBAC checks scope match. | `RbacDecision::Deny` if scope mismatch |
| P4 | Two-person approval | `PolicyEngine::evaluate()` | If `VClusterPolicy.two_person_approval == true` AND state-changing → `RequireApproval`. Approver != requester checked on approve. | `PolicyDecision::RequireApproval` |
| P5 | Approval timeout | `PendingApproval.expires_at` | Checked when approval is submitted. Expired approvals rejected. | `ApprovalStatus::Expired` |
| P6 | Platform admin always authorized | `RbacEngine::evaluate()` | Early return `Allow` for `pact-platform-admin` role. Still logged (O3). | Allow (but log) |
| P7 | Degraded mode restrictions | `PolicyCache::evaluate_degraded()` | Separate code path when PolicyService unreachable. Fail-closed for two-person and OPA rules. | Deny complex operations, allow cached whitelist |
| P8 | AI agent emergency restriction | `PolicyEngine::evaluate()` | Check `identity.role == "pact-service-ai"` AND `action == "emergency"` → Deny | `PolicyDecision::Deny` |

---

## Shell & Exec Invariants

| ID | Invariant | Enforcement Point | Mechanism | Violation Response |
|----|-----------|-------------------|-----------|-------------------|
| S1 | Whitelist enforcement | `ShellService::exec()` / shell PATH | Exec: command checked against `VClusterPolicy.exec_whitelist`. Shell: PATH restricted to whitelisted dirs (ADR-007). | Exec: `PERMISSION_DENIED`. Shell: "command not found" |
| S2 | Platform admin whitelist bypass | `ShellService::exec()` | If role == `pact-platform-admin`, skip whitelist check. Command still logged. | Allow + log |
| S3 | Restricted bash | Shell session setup | Shell spawned as `rbash` with restricted PATH, no redirects, no absolute paths | Bash restriction enforcement |
| S4 | Session audit | `ShellService::exec()` and shell `PROMPT_COMMAND` | Every command → `AdminOperation` entry in journal | Automatic via PROMPT_COMMAND hook |
| S5 | State-changing commands trigger window | Agent drift observer | Shell does NOT pre-classify (S6). Observer detects actual filesystem/config changes post-execution. | DriftEvent → CommitWindow (normal drift flow) |
| S6 | No pre-classification | `ShellService` design | Shell passes commands directly to bash. No command parsing or classification. Drift detection handles state changes. | By design — observer detects, not shell |

---

## Observability Invariants

| ID | Invariant | Enforcement Point | Mechanism | Violation Response |
|----|-----------|-------------------|-----------|-------------------|
| O1 | No per-agent Prometheus | Agent design | No metrics HTTP endpoint in agent. Telemetry flows through journal only (ADR-005). | Structural — no endpoint to scrape |
| O2 | Journal metrics port 9091 | `TelemetryServer` (axum) | Hardcoded bind to `:9091`. Avoids Prometheus default 9090 conflict. | Config validation at startup |
| O3 | Audit trail continuity | `JournalState.audit_log` | Append-only Vec. No delete/truncate method. Emergency/degraded modes still append. | Structural — no removal path |

---

## Federation Invariants

| ID | Invariant | Enforcement Point | Mechanism | Violation Response |
|----|-----------|-------------------|-----------|-------------------|
| F1 | Config state is site-local | `FederationSync` trait | Sync only pulls Rego templates from Sovra. No method to push config/drift/audit data. | Structural — no export API |
| F2 | Policy templates federated | `FederationSync::sync()` | Pulls Rego templates on interval (default 300s). Stores locally for OPA. | Templates cached and used locally |
| F3 | Graceful federation failure | `FederationSync::sync()` error handling | On Sovra unreachable, continue with cached templates. Log warning. No functionality lost. | Degraded: cached templates |

---

## Node Management Invariants

| ID | Invariant | Enforcement Point | Mechanism | Violation Response |
|----|-----------|-------------------|-----------|-------------------|
| NM-I1 | One backend per deployment | `DelegationConfig.node_mgmt_backend` | Single enum field (`Csm` or `Ochami`), not per-node | Structural — enum has one value |
| NM-I2 | Audit before delegation | `delegate.rs:reboot_node/reimage_node` | `audit_delegation()` called before `backend.reboot/reimage()` | Audit entry exists even on backend failure |
| NM-I3 | Graceful failure | `NodeMgmtError` enum | All backend errors mapped to typed variants, no panics | Error message returned to CLI user |
| NM-I4 | Single credential scope | `DelegationConfig` | One `node_mgmt_base_url` + `node_mgmt_token` used for HSM and power APIs | Structural — single config source |
| NM-I5 | Uniform CLI semantics | `NodeManagementBackend` trait | `reimage()` has identical signature regardless of backend. No CSM-specific params leak. | Compile-time — trait enforces contract |

---

## Raft Invariants

| ID | Invariant | Enforcement Point | Mechanism | Violation Response |
|----|-----------|-------------------|-----------|-------------------|
| R1 | Independent Raft groups | Deployment architecture | Separate openraft instances, separate state machines, separate WAL directories | Configuration — validated at startup |
| R2 | Pact is incumbent | Boot sequence | pact-journal starts before lattice in co-located mode (system service ordering) | Deployment constraint |
| R3 | Quorum ports | Configuration | Pact: Raft 9444, gRPC 9443. Lattice: Raft 9000, gRPC 50051. | Config validation at startup |

---

## Conflict Resolution Invariants

| ID | Invariant | Enforcement Point | Mechanism | Violation Response |
|----|-----------|-------------------|-----------|-------------------|
| CR1 | Local changes fed back first | `Agent::on_reconnect()` | Agent sends pending local entries via `ConfigService.AppendEntry` before re-subscribing to config stream | Structural — reconnect protocol enforces order |
| CR2 | Merge conflict pauses convergence | `Agent::on_reconnect()` + `JournalState::detect_conflicts()` | Journal returns conflict manifest when local entries conflict with current state on same keys. Agent pauses convergence for conflicting keys. | Agent enters `ConflictPending` state for affected keys |
| CR3 | Grace period fallback | `ConflictManager::tick()` | Timer started when conflict detected. If expired without admin resolution → journal-wins. Overwritten values logged. | Auto-resolve + audit log entry |
| CR4 | Promote requires conflict ack | `CLI::promote()` + `JournalState::check_promote_conflicts()` | Before applying promote, journal checks all target nodes for local changes on overlapping keys. Returns conflict manifest if any. CLI blocks until admin resolves. | Promote blocked until conflicts resolved |
| CR5 | Admin notification on overwrite | `CLI::session_notify()` | Active CLI sessions receive notification via gRPC stream when their uncommitted/local changes are overwritten by promote or grace period timeout | Notification delivered (best-effort) |
| CR6 | No cross-vCluster atomicity | `ConfigService.AppendEntry` | Each entry scoped to single vCluster via `Scope`. No multi-vCluster transaction API exists. | Structural — no API for atomic cross-vCluster ops |

---

## Node Delta Invariants

| ID | Invariant | Enforcement Point | Mechanism | Violation Response |
|----|-----------|-------------------|-----------|-------------------|
| ND1 | TTL minimum 15 min | `JournalState::apply(AppendEntry)` | Validate `ttl.unwrap_or(0) == 0 \|\| ttl >= 900` | `JournalResponse::ValidationError { reason: "TTL must be >= 900 seconds (15 minutes)" }` |
| ND2 | TTL maximum 10 days | `JournalState::apply(AppendEntry)` | Validate `ttl.unwrap_or(0) == 0 \|\| ttl <= 864000` | `JournalResponse::ValidationError { reason: "TTL must be <= 864000 seconds (10 days)" }` |
| ND3 | vCluster homogeneity warning | `CLI::status()` + `JournalState::check_homogeneity()` | When querying status, journal checks for per-node deltas that deviate from vCluster overlay. Reports divergent nodes. | Warning in CLI output (not a hard error) |

---

## Enrollment & Certificate Invariants (ADR-008)

| ID | Invariant | Enforcement Point | Mechanism | Violation Response |
|----|-----------|-------------------|-----------|-------------------|
| E1 | No connection without enrollment | `EnrollmentService::enroll()` | Hardware identity matched against enrollment registry. No match → reject. Only unauthenticated gRPC method. | `PactError::NodeNotEnrolled` |
| E2 | Hardware identity uniqueness | `JournalState::apply(EnrollNode)` | Index of `HardwareIdentity → NodeId` in journal state. Duplicate MAC+BMC serial → reject. | `PactError::HardwareIdentityConflict` |
| E3 | Single activation across domains | Physical constraint | Node can PXE boot from only one Manta at a time. No distributed lock needed. | Advisory: Sovra cross-domain visibility if federated |
| E4 | CSR model — no private keys in journal | `CaKeyManager::sign_csr()` + `EnrollmentService::enroll()` | Agent generates keypair, sends CSR. Journal signs locally with intermediate CA key. Only signed cert (public) stored in Raft. No private key material anywhere in journal. | Structural — no field or API accepts private keys |
| E5 | Cert lifetime and renewal | `EnrollmentService::renew_cert()` + `CaKeyManager::sign_csr()` | Agent generates new keypair + CSR at 2/3 of cert lifetime. Journal signs locally. No batch sweep needed. | Agent retries if journal unreachable; active channel continues (F19) |
| E6 | Dual-channel rotation | `DualChannelClient::rotate()` | Passive channel built with new cert, health-checked, atomically swapped. Old channel drains. | `EnrollmentError::RotationFailed` → active channel continues |
| E7 | Enrollment state governs CSR signing | `EnrollmentService::enroll()` | State machine check: Registered or Inactive → sign CSR. Active → reject ALREADY_ACTIVE (prevents race). Revoked → reject. | `PactError::NodeRevoked` or `PactError::AlreadyActive` |
| E8 | vCluster independent of enrollment | `JournalState` data model | `NodeEnrollment.vcluster_id: Option<VClusterId>`. Assignment operations are separate Raft commands. Cert CN has no vCluster. | Structural — separate fields, separate operations |
| E9 | Decommission revokes cert | `EnrollmentService::decommission()` | Sets state to Revoked. Adds serial to Raft revocation registry. Journal nodes check registry on mTLS handshake. | `PactError::NodeRevoked` on subsequent access |
| E10 | Only platform-admin can enroll/decommission | gRPC interceptor + `RbacEngine` | `RegisterNode`, `DecommissionNode` require `pact-platform-admin`. `AssignNode` allows `pact-ops-{vc}` for their vCluster. | `PactError::Unauthorized` |

---

## Authentication Invariants (hpc-auth crate)

| ID | Invariant | Enforcement Point | Mechanism | Violation Response |
|----|-----------|-------------------|-----------|-------------------|
| Auth1 | No unauth commands | `AuthClient::get_token()` → consumer check | Consumer calls `get_token()` before gRPC. Error → exit with "run login" message. login/logout/version/help exempt. | `AuthError::TokenExpired` → exit code 2 |
| Auth2 | Fail closed on cache corruption | `TokenCache::read()` | Validates JSON structure. Invalid → `AuthError::CacheCorrupted`. Never attempts partial parse. | Cache rejected, user must re-login |
| Auth3 | Concurrent refresh safe | `AuthClient::get_token()` | File lock on cache during refresh. Refresh is idempotent at IdP. Last writer wins. | No conflict — idempotent |
| Auth4 | Logout always clears local | `AuthClient::logout()` | Deletes cache entry BEFORE attempting IdP revocation. IdP failure does not block cache clear. | Cache always cleared |
| Auth5 | Cache file 0600 permissions | `TokenCache::read()` + `TokenCache::write()` | Read: checks permissions per `PermissionMode` (strict=reject, lenient=warn+fix). Write: always creates with 0600. | Strict: `AuthError::CachePermissionDenied`. Lenient: warn + fix |
| Auth6 | Per-server token isolation | `TokenCache` keying | Cache file is a JSON map keyed by server URL. No cross-server access. | Structural — HashMap<server_url, tokens> |
| Auth7 | Refresh tokens never logged | `TokenSet::fmt()` + logging | `Display` impl redacts refresh_token field. `tracing` instrumentation excludes it. | Structural — no log path includes refresh token |
| Auth8 | Cascading flow fallback | `AuthClient::login()` | Probes IdP discovery for supported grants. Tries PKCE → Confidential → DeviceCode → ManualPaste in order. | Falls back or `AuthError::NoSupportedFlow` |

## Authentication Invariants (PACT-specific consumer)

| ID | Invariant | Enforcement Point | Mechanism | Violation Response |
|----|-----------|-------------------|-----------|-------------------|
| PAuth1 | Strict permission mode | `pact-cli` AuthClient construction | `PermissionMode::Strict` passed to hpc-auth. Cache with wrong perms rejected. | Security error + "run pact login" |
| PAuth2 | Emergency requires human | `PolicyEngine::evaluate()` + CLI auth check | P8 enforced server-side. CLI also checks principal_type != Service/Agent before sending. | Authorization error |
| PAuth3 | Auth discovery public | `pact-journal` telemetry server | `/auth/discovery` endpoint on port 9091, no auth middleware. Returns IdP URL + client_id. | Structural — no auth check on route |
| PAuth4 | Break-glass is BMC | Error message in `pact login` | When IdP unreachable + tokens expired, error suggests BMC console access. No pact break-glass mechanism. | Info message in error output |
| PAuth5 | Two-person distinct identities | `PolicyService.DecideApproval()` | Compares `requester.principal` vs `approver.principal`. Same-identity → reject. | `ValidationError: self-approval denied` |

---

## Process Supervision Invariants

| ID | Invariant | Enforcement Point | Mechanism | Violation Response |
|----|-----------|-------------------|-----------|-------------------|
| PS1 | Adaptive supervision loop | `PactSupervisor::run_loop()` | Check `cgroup_manager.is_scope_empty(workload_slice)` each tick. Adjust interval. Coupled to watchdog pet. | Automatic adaptation |
| PS2 | Watchdog coupled to loop | `PactSupervisor::run_loop()` | `watchdog.pet()` called in loop body. If loop hangs → no pet → BMC reboot. | F23: BMC hard reboot |
| PS3 | cgroup scope kills children | `CgroupManager::destroy_scope()` | Write to `cgroup.kill` before removing scope dir. Timeout 10s → log zombie scope (F30). | AuditEvent + zombie scope logged |

---

## Resource Isolation Invariants

| ID | Invariant | Enforcement Point | Mechanism | Violation Response |
|----|-----------|-------------------|-----------|-------------------|
| RI1 | Exclusive slice ownership | `CgroupManager::create_scope()` | Check `slice_owner(parent)`. Reject if caller doesn't own slice. Emergency override checked separately (RI3). | `CgroupError::PermissionDenied` |
| RI2 | Every process has scope | `PactSupervisor::start_service()` | Calls `cgroup_manager.create_scope()` BEFORE `Command::spawn()`. Process placed in scope via cgroup.procs. | Service start fails if scope creation fails (F22) |
| RI3 | Emergency override + audit | `EmergencySession::freeze_workload()` | Check emergency state active + OIDC auth. Emit AuditEvent BEFORE freeze/kill. | `PactError::EmergencyRequired` if not in emergency |
| RI4 | pact-agent OOM protection | `boot::init_hardware()` | Write -1000 to `/proc/self/oom_score_adj` during InitHardware phase | Structural — set once at boot |
| RI5 | CgroupHandle callback | `PactSupervisor::start_service()` | On spawn failure: explicit `cgroup_manager.destroy_scope(handle)` in error path | Structural — error handling code |
| RI6 | Shared read across slices | `CgroupManager::read_metrics()` | No ownership check on reads. Any path readable. | By design — read method has no owner check |

---

## Identity Mapping Invariants

| ID | Invariant | Enforcement Point | Mechanism | Violation Response |
|----|-----------|-------------------|-----------|-------------------|
| IM1 | UID stable within federation | `JournalState::apply(AssignUid)` | UidEntry keyed by (org, subject). Updates rejected (append-only per subject). GC only on federation departure. | `JournalResponse::ValidationError` on duplicate |
| IM2 | Precursor ranges no overlap | `JournalState::apply(AddOrgIndex)` | Sequential org_index assignment. `precursor = base + index * stride`. No manual range config. | Structural — sequential index |
| IM3 | Sequential within range | `JournalState::apply(AssignUid)` | Counter per org tracks next UID. If counter >= stride → exhaustion error. | `PactError::UidRangeExhausted` + alert (F24) |
| IM4 | Pre-provisioned rejects unknowns | `PolicyEngine::evaluate()` (identity check) | If `vcluster_policy.identity_mode == PreProvisioned` AND subject not in UidMap → deny | `PactError::IdentityNotProvisioned` |
| IM5 | NSS module read-only | `libnss_pact.so` implementation | mmap on .db files. No write syscalls, no socket syscalls. | Structural — code has no write/network capability |
| IM6 | Only in PactSupervisor mode | `boot::load_identity()` | Skip .db file creation if `config.supervisor.backend == Systemd` | Structural — conditional code path |
| IM7 | UidMap before non-root services | Boot phase ordering (PB3) + `PactSupervisor::start_service()` | Phase 3 before Phase 5. Runtime: check getpwnam() succeeds before spawn for non-root services. | Service start waits/retries until UidMap available |

---

## Network Management Invariants

| ID | Invariant | Enforcement Point | Mechanism | Violation Response |
|----|-----------|-------------------|-----------|-------------------|
| NM1 | Netlink only in PactSupervisor | `boot::configure_network()` | Skip if `config.supervisor.backend == Systemd` | Structural — conditional code path |
| NM2 | Network before services | Boot phase ordering (PB3) | Phase 2 (ConfigureNetwork) before Phase 5 (StartServices) | Boot blocked on network failure (F28) |

---

## Platform Bootstrap Invariants

| ID | Invariant | Enforcement Point | Mechanism | Violation Response |
|----|-----------|-------------------|-----------|-------------------|
| PB0 | Pseudofs before /proc readers | `PlatformInit::mount_pseudofs()` | Called as first action in Phase 0, before `protect_from_oom()` or any /proc access. Uses devtmpfs for /dev (not tmpfs). Idempotent — checks /proc/mounts. | Phase 0 fails → PB3 blocks all subsequent phases |
| PB1 | Watchdog only as PID 1 | `boot::init_hardware()` | Check `getpid() == 1` AND `/dev/watchdog` exists. EBUSY → `WatchdogBusy` (non-fatal, degraded). Skip otherwise. | No watchdog opened in non-PID-1 mode. EBUSY logged as warning. |
| PB2 | Watchdog pet interval | `PactSupervisor::run_loop()` + `WatchdogHandle::spawn_boot_petter()` | During boot: dedicated petter task at T/2 interval. After boot: supervision loop pets via `as_pet_callback()`. Boot petter aborted when loop starts. | Coupled to PS2. Pet failure logged, never panics — failed pet = eventual BMC reboot (F23). |
| PB3 | Strict boot phase ordering | `BootSequence::run()` | Sequential execution. Phase N+1 starts only after Phase N returns Ok. | `BootPhase::BootFailed` on error, blocks subsequent |
| PB4 | Bootstrap identity temporary | `identity::on_svid_acquired()` | On SVID acquisition: clear bootstrap cert from memory, log discard | Structural — overwrite + drop |
| PB5 | No hard SPIRE dependency | `IdentityCascade::get_identity()` | SPIRE is first in cascade. If unavailable, falls through to SelfSigned, then Bootstrap. | Automatic fallback (IdentityCascade) |

---

## Workload Integration Invariants

| ID | Invariant | Enforcement Point | Mechanism | Violation Response |
|----|-----------|-------------------|-----------|-------------------|
| WI1 | Unix socket only for handoff | `handoff::HandoffServer` | Listens on `HANDOFF_SOCKET_PATH`. SCM_RIGHTS for FD passing. No other transport. | Structural — only unix socket code exists |
| WI2 | Refcount accuracy | `MountManager` impl | `acquire_mount` increments, `release_mount` decrements. Assert refcount >= 0. | Assert failure + AuditEvent on negative (F31) |
| WI3 | Lazy unmount + hold timer | `MountManager::release_mount()` | On refcount=0: start timer. On timer expiry: unmount. Emergency `--force` bypasses timer. | Timer-based. Emergency override checked via EmergencySession state. |
| WI4 | Lattice standalone creates own | hpc-node trait design | Same `CgroupManager` trait implemented by both. Lattice's impl creates hierarchy if pact hasn't. | By design — trait-based contract |
| WI5 | Cleanup on cgroup empty | `PactSupervisor::run_loop()` (idle tick) | Poll allocation cgroups with `is_scope_empty()`. On empty → cleanup NamespaceSet + release mounts. | Automatic on idle ticks |
| WI6 | Refcount reconstruction | `MountManager::reconstruct_state()` | On agent restart: read `/proc/mounts`, cross-reference with journal active allocations, rebuild refcount map. | Called during boot after journal reconnect |

---

## Capability Detection Invariants

| ID | Invariant | Enforcement Point | Mechanism | Violation Response |
|----|-----------|-------------------|-----------|-------------------|
| CAP1 | CPU arch accuracy | `LinuxCpuBackend::detect()` | Parses `/proc/cpuinfo` "model name" field + `uname -m` (via `std::env::consts::ARCH`). Maps to `CpuArchitecture` enum. | Reports `CpuArchitecture::Unknown` if `/proc/cpuinfo` unreadable or arch string unrecognised (F37) |
| CAP2 | Memory total matches /proc/meminfo | `LinuxMemoryBackend::detect()` | Parses `/proc/meminfo` `MemTotal` line, converts kB to bytes. NUMA topology from `/sys/devices/system/node/node*/meminfo`. Hugepages from `/proc/meminfo` HugePages_* lines. Memory type via optional `dmidecode --type 17` (graceful fallback to `MemoryType::Unknown`). | Reports `total_bytes=0`, `available_bytes=0` if `/proc/meminfo` unreadable (F37). Reports `numa_nodes=1` with single-node fallback if `/sys/devices/system/node/` unavailable (F38). |
| CAP3 | NIC count matches physical | `LinuxNetworkBackend::detect()` | Enumerates `/sys/class/net/*/`, filters loopback (`lo`) and virtual interfaces (no `/sys/class/net/*/device` symlink). Per interface: reads `speed`, `operstate`, `address`, detects fabric from driver symlink (`cxi` → Slingshot, others → Ethernet). | Reports empty `Vec<NetworkInterface>` if `/sys/class/net/` unreadable. Reports `speed_mbps=0` for interfaces where `/sys/class/net/*/speed` returns -1 or is unreadable (F39). |
| CAP4 | Storage uses statvfs() | `LinuxStorageBackend::detect()` | Determines `StorageNodeType` from presence of `/sys/block/nvme*` or `/sys/block/sd*`. Enumerates local disks from `/sys/block/*/`. Parses `/proc/mounts` for active mounts. Calls `nix::sys::statvfs::statvfs()` per mount for real capacity (`total_bytes`, `available_bytes`). | Reports `total_bytes=0`, `available_bytes=0` on `statvfs` failure per mount (F41). Mount still listed with path/fs_type/source. Reports `StorageNodeType::Diskless` with `local_disks=[]` if `/sys/block/` unreadable (F40). |
| CAP5 | Non-Linux compiles | `MockCpuBackend`, `MockMemoryBackend`, `MockNetworkBackend`, `MockStorageBackend` | All Mock backends always compiled (no `#[cfg]` gate). Store configurable test data via constructor (`with_*` pattern, matching `MockGpuBackend::with_gpus`). Return stored data from `detect()`. Linux backends are `#[cfg(target_os = "linux")]` only. | Structural — Mock impls have no platform-specific dependencies. `CapabilityReporter` accepts `Box<dyn XxxBackend>` for all 5 categories, enabling macOS development and deterministic testing. |

---

## Diagnostic Log Retrieval Invariants

| ID | Invariant | Enforcement Point | Mechanism | Violation Response |
|----|-----------|-------------------|-----------|-------------------|
| LOG1 | Diagnostic log retrieval authorization | `ShellServiceImpl::collect_diag()` | Same auth check as exec: extract OIDC token from gRPC metadata (`extract_auth`), validate via `has_ops_role(identity, vcluster)`. `pact-platform-admin` bypasses via P6. Viewers rejected. | `tonic::Status::PERMISSION_DENIED` |
| LOG2 | Server-side grep filtering | `diag.rs::collect_from_source()` | Grep pattern applied per-line on agent before adding to `DiagChunk.lines`. Only matching lines transmitted. Empty pattern = no filtering. | Structural — grep runs before response construction |
| LOG3 | Agent-side line limit enforcement | `diag.rs::collect_from_source()` | Line count checked per source. Default 100, max 10000. Requests with `line_limit > 10000` rejected with `INVALID_ARGUMENT`. `DiagChunk.truncated = true` when limit reached. | `tonic::Status::INVALID_ARGUMENT` for over-limit requests. Truncation flag for capped output. |

---

## Enforcement Categories

Summary of how invariants are enforced:

| Category | Count | Invariants | Description |
|----------|-------|------------|-------------|
| **Structural** | 29 | J2, J6, J7, J8, J9, D2, O1, O3, F1, S6, CR1, CR6, ND3, E3, E8, RI4, RI5, RI6, IM2, IM5, IM6, NM1, PB4, WI1, WI2(assert), WI4, PB1, CAP5, LOG2 | Impossible to violate by design (no API exists to break them) |
| **Validation** | 15 | J3, J4, J5, A3, D3, P5, O2, ND1, ND2, E1, E2, E7, IM1, IM3, LOG3 | Checked at input boundary, rejected with error |
| **Runtime logic** | 40 | A1-A2, A4-A6, A9-A10, D1, D4-D5, P1-P4, P6-P8, S1-S5, CR2, CR4, CR5, E4-E6, E9-E10, PS1-PS3, RI1-RI3, IM4, IM7, NM2, WI3, WI5, WI6, CAP1-CAP4, LOG1 | Active enforcement in business logic |
| **Operational** | 5 | A7, A8, R1-R3 | Monitored/configured, not enforced in code |
| **Protocol** | 2 | J1, J9 | Guaranteed by Raft consensus protocol |
| **Degraded fallback** | 7 | P7, F2, F3, A9, CR3, PB5, PB2 | Special behavior when components unavailable |
| **Boot ordering** | 2 | PB3, NM2 | Enforced by sequential boot phase execution |
