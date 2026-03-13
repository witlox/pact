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
