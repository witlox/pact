Feature: Cross-Context Integration
  Scenarios that span multiple bounded contexts and verify
  interactions between journal, agent, policy, CLI, and external systems.

  Background:
    Given a journal with default state
    And node "node-001" is assigned to vCluster "ml-training"
    And a supervisor with backend "pact"
    And a shell server with default whitelist

  # --- Boot → Config → Service lifecycle ---

  Scenario: Full boot sequence streams config and starts services
    Given vCluster "ml-training" has an overlay version 1
    And vCluster "ml-training" declares services "chronyd,lattice-node-agent"
    When node "node-001" boots and authenticates to journal
    Then node "node-001" receives overlay version 1
    And node "node-001" receives its node delta
    And services start in dependency order: "chronyd" then "lattice-node-agent"
    And a CapabilityReport is written to tmpfs
    And node "node-001" subscribes to config updates

  Scenario: Config update triggers service restart
    Given node "node-001" has booted and is subscribed to config updates
    When the overlay for vCluster "ml-training" is updated with new service config
    Then node "node-001" receives the config update via subscription
    And affected services are restarted in dependency order

  # --- Drift → Commit Window → Journal ---

  Scenario: Drift detected by observer triggers commit window and journal entry
    Given node "node-001" is in state "Committed"
    And enforcement mode is "enforce"
    When a kernel parameter change is detected by eBPF observer
    Then a DriftDetected entry is recorded in the journal
    And a commit window opens with duration based on drift magnitude
    And node "node-001" state changes to "Drifted"

  Scenario: Commit window expiry triggers rollback and journal entry
    Given node "node-001" is in state "Drifted" with an active commit window
    When the commit window expires
    Then auto-rollback is attempted
    And a Rollback entry is recorded in the journal
    And node "node-001" state changes to "Committed"

  # --- Exec → Policy → Audit ---

  Scenario: Exec command flows through policy and audit
    Given user "admin@example.com" has role "pact-ops-ml-training"
    And "nvidia-smi" is in the exec whitelist for vCluster "ml-training"
    When admin executes "nvidia-smi" on node "node-001"
    Then PolicyService.Evaluate is called with action "exec"
    And the command is fork/exec'd on node "node-001"
    And stdout is streamed back to the CLI
    And an ExecLog entry is recorded in the journal with full command and output

  Scenario: Non-whitelisted exec denied without policy call
    Given user "admin@example.com" has role "pact-ops-ml-training"
    And "rm" is NOT in the exec whitelist for vCluster "ml-training"
    When admin executes "rm /tmp/test" on node "node-001"
    Then the request is rejected with exit code 6
    And PolicyService.Evaluate is NOT called
    And the denial is logged in the journal

  # --- Shell → Drift → Commit ---

  Scenario: Shell session state change triggers drift detection
    Given user "admin@example.com" has an active shell session on node "node-001"
    When the admin runs "sysctl -w net.core.somaxconn=1024" in the shell
    Then the drift observer detects the kernel parameter change
    And a commit window opens
    And the shell session continues uninterrupted

  # --- Emergency → Scheduling Hold → Force-End ---

  Scenario: Emergency escalation through scheduling hold
    Given user "admin@example.com" has role "pact-ops-ml-training"
    And vCluster "ml-training" has emergency_allowed true
    When admin enters emergency mode on node "node-001" with reason "GPU debugging"
    Then an EmergencyStart entry is recorded in the journal
    And node "node-001" state changes to "Emergency"
    When the emergency window expires without ending
    Then a Loki alert event is sent
    And lattice is called to cordon node "node-001"
    When admin "ops-lead@example.com" force-ends the emergency
    Then an EmergencyEnd entry is recorded in the journal
    And node "node-001" state returns to "Committed" or "Drifted"

  # --- Two-Person Approval Flow ---

  Scenario: Regulated vCluster commit requires two-person approval
    Given vCluster "regulated-hpc" has two-person approval enabled
    And user "alice@example.com" has role "pact-regulated-regulated-hpc"
    When alice commits a config change on vCluster "regulated-hpc"
    Then PolicyService returns approval_required
    And a PendingApproval entry is created in the journal
    When user "bob@example.com" approves the pending operation
    Then the commit is applied through Raft
    And both alice and bob are recorded in the audit log

  Scenario: Self-approval is denied
    Given vCluster "regulated-hpc" has two-person approval enabled
    And user "alice@example.com" has role "pact-regulated-regulated-hpc"
    When alice commits a config change on vCluster "regulated-hpc"
    And alice attempts to approve her own pending operation
    Then the approval is rejected
    And the pending operation remains pending

  # --- Partition → Degraded → Conflict Resolution → Replay ---

  Scenario: Agent operates during partition and replays on reconnect
    Given node "node-001" has cached config and policy for vCluster "ml-training"
    And the journal is unreachable from node "node-001"
    When a drift event occurs on node "node-001"
    Then drift is logged locally on node "node-001"
    And cached policy is used for authorization
    When the network partition heals
    Then node "node-001" reconnects to the journal
    And locally logged events are replayed to the journal
    And config subscription resumes from last known sequence

  Scenario: Partition with local admin changes triggers merge conflict on reconnect
    Given node "node-001" has cached config and policy for vCluster "ml-training"
    And the journal is unreachable from node "node-001"
    When an admin changes "net.core.somaxconn" to "2048" on node "node-001" via pact shell
    And meanwhile "net.core.somaxconn" is committed as "4096" in the journal for vCluster "ml-training"
    And the network partition heals
    Then node "node-001" reports its local change to the journal first
    And a merge conflict is detected on "net.core.somaxconn"
    And node "node-001" pauses convergence for "net.core.somaxconn"
    When admin "ops@example.com" resolves by accepting journal value
    Then node "node-001" applies "net.core.somaxconn" as "4096"
    And the overwritten local value "2048" is logged for audit
    And config subscription resumes normally

  # --- Promote → Overlay Update → Boot ---

  Scenario: Promote node delta to overlay affects subsequent boots
    Given node "node-001" has committed deltas (sysctl changes)
    When admin runs "pact promote node-001"
    Then the deltas are exported as overlay TOML
    When admin runs "pact apply" with the exported TOML
    Then the overlay for vCluster "ml-training" is rebuilt
    And subscribed agents receive the overlay update
    When a new node boots into vCluster "ml-training"
    Then the new node receives the updated overlay including the promoted changes

  # --- GPU Failure → Capability → Scheduler ---

  Scenario: GPU degradation flows from agent to scheduler
    Given node "node-001" has 4 NVIDIA GPUs all healthy
    When GPU index 2 health degrades to "Degraded"
    Then the CapabilityReport is updated immediately
    And a CapabilityChange entry is recorded in the journal
    And the tmpfs manifest is updated
    And lattice-node-agent reads the updated manifest

  # --- Federation → Policy Update ---

  Scenario: Federated policy template affects local evaluation
    Given Sovra provides an updated Rego template for "exec authorization"
    When the federation sync interval fires
    Then the template is pulled and stored locally
    And OPA receives the updated bundle
    When admin executes a command requiring the updated policy
    Then OPA evaluates using the new template
    And the result reflects the updated rules

  # --- Boot → cgroup → Service → Watchdog lifecycle ---

  Scenario: Full PID 1 boot creates cgroups, starts services, pets watchdog
    Given a supervisor with backend "pact"
    And /dev/watchdog is available
    When pact-agent boots as PID 1
    Then InitHardware should mount cgroup2 and create slice hierarchy
    And ConfigureNetwork should configure interfaces via netlink
    And LoadIdentity should load UidMap and write .db files
    And PullOverlay should stream vCluster overlay from journal
    And StartServices should create cgroup scopes and start services in order
    And ReadinessSignal should be emitted
    And the supervision loop should be running
    And each loop tick should pet the hardware watchdog

  # --- Supervision → Isolation → Cleanup chain ---

  Scenario: Service crash triggers supervision restart with cgroup cleanup
    Given a running service "nvidia-persistenced" in cgroup scope "pact.slice/gpu.slice/nvidia-persistenced"
    And "nvidia-persistenced" has forked 2 child processes
    When the main process of "nvidia-persistenced" crashes
    Then the supervision loop detects the crash
    And Resource Isolation kills all processes in the cgroup scope
    And the cgroup scope is released
    And a new cgroup scope is created for the restart
    And "nvidia-persistenced" is restarted in the new scope
    And an AuditEvent records the crash and restart

  # --- Identity → Supervision → NFS chain ---

  Scenario: Service running as non-root user requires identity mapping
    Given identity_mode is "on-demand"
    And a service "app-svc" declared with user "appuser"
    And "appuser@cscs.ch" has been assigned UID 10042
    When pact-agent boots and reaches StartServices phase
    Then UidMap should already be loaded (Phase 3 before Phase 5)
    And getpwnam("appuser") should resolve to UID 10042
    And "app-svc" should start as UID 10042 in its cgroup scope
    And NFS files created by "app-svc" should be owned by UID 10042

  # --- Namespace handoff → Mount sharing → Allocation lifecycle ---

  Scenario: Allocation lifecycle with namespace handoff and shared mount
    Given pact-agent is running with ReadinessSignal emitted
    And lattice-node-agent is running and connected via unix socket
    When lattice requests allocation "alloc-01" with uenv "pytorch-2.5.sqfs"
    Then pact creates pid/net/mount namespaces for "alloc-01"
    And pact mounts "pytorch-2.5.sqfs" (MountRef refcount=1)
    And pact bind-mounts into alloc-01's mount namespace
    And namespace FDs are passed to lattice via SCM_RIGHTS
    When lattice requests allocation "alloc-02" with same uenv "pytorch-2.5.sqfs"
    Then no new SquashFS mount occurs (MountRef refcount=2)
    And alloc-02 gets its own namespaces and bind-mount
    When alloc-01 completes (cgroup empties)
    Then pact detects empty cgroup and cleans up alloc-01's namespaces
    And MountRef refcount decreases to 1
    When alloc-02 completes (cgroup empties)
    Then pact cleans up alloc-02's namespaces
    And MountRef refcount reaches 0
    And cache hold timer starts for "pytorch-2.5.sqfs"

  # --- Emergency → workload.slice override → audit ---

  Scenario: Emergency mode allows cross-slice intervention with full audit
    Given node "node-001" has active workloads in workload.slice
    And user "admin@example.com" has role "pact-ops-ml-training"
    When admin enters emergency mode with reason "runaway process consuming all memory"
    Then an EmergencyStart AuditEvent is recorded
    When admin requests freeze of workload.slice with "--force"
    Then pact freezes all processes in workload.slice
    And an EmergencyFreeze AuditEvent is recorded with admin identity
    And any mount hold timers in workload.slice are overridden
    When admin ends emergency mode with commit
    Then an EmergencyEnd AuditEvent is recorded
    And workload.slice processes remain frozen (lattice must restart them)

  # --- SPIRE bootstrap → mTLS rotation ---

  Scenario: SPIRE SVID replaces bootstrap identity during boot
    Given pact-agent starts with bootstrap identity from OpenCHAMI
    And SPIRE agent is running on the node
    When pact-agent authenticates to journal using bootstrap identity
    And pact-agent connects to SPIRE agent socket
    Then SPIRE issues an SVID for pact-agent workload
    And pact-agent rotates mTLS to use SVID (dual-channel swap)
    And the bootstrap identity is discarded
    And all subsequent journal communication uses SPIRE-managed mTLS

  # --- Agent crash → mount reconstruction → allocation continuity ---

  Scenario: Agent crash preserves running allocations
    Given 2 active allocations using "pytorch-2.5.sqfs" (MountRef refcount=2)
    And allocation workload processes are running in their cgroup scopes
    When pact-agent crashes
    Then workload processes continue running (orphaned but alive in cgroups)
    When pact-agent restarts
    Then pact scans kernel mount table and finds "pytorch-2.5.sqfs" mounted
    And pact queries journal for active allocations on this node
    And MountRef is reconstructed with refcount=2
    And supervision loop resumes monitoring supervised services
    And namespace handoff socket is re-opened for lattice

  # --- Federation UID lifecycle ---

  Scenario: Federated user gets UID, uses NFS, org departs
    Given org "partner-a" joined with org_index 1 (precursor 20000, stride 10000)
    And "researcher@partner-a.org" authenticates via OIDC
    Then "researcher@partner-a.org" is assigned UID 20000 in the journal
    And all agents receive the UidMap update
    And NFS files created by this user are owned by UID 20000
    When org "partner-a" leaves federation
    Then all UidEntries for "partner-a" are GC'd from journal
    And agents remove "partner-a" entries from .db files
    And NFS files owned by UID 20000 become orphaned (numeric only)
    And org_index 1 becomes reclaimable

  # --- Systemd compat mode skips init-specific interactions ---

  Scenario: Systemd mode disables pact-specific sub-contexts
    Given a supervisor with backend "systemd"
    When pact-agent starts
    Then no hardware watchdog is opened
    And no netlink interface configuration occurs
    And no UidMap .db files are written
    And no cgroup slices are created by pact
    And systemd manages service restart natively
    And pact still pulls overlay and manages config state
