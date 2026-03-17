Feature: Resource Isolation
  pact-agent manages cgroup v2 hierarchy for service isolation, OOM containment,
  and namespace creation for lattice allocations. cgroup slices define ownership
  boundaries between pact and lattice. (Invariants: RI1-RI6, PS3)

  Background:
    Given a supervisor with backend "pact"
    And the cgroup v2 filesystem is mounted

  # --- Slice hierarchy ---

  Scenario: Boot creates cgroup slice hierarchy
    When pact-agent completes InitHardware boot phase
    Then the following cgroup slices should exist:
      | slice                        | owner   |
      | pact.slice/infra.slice       | pact    |
      | pact.slice/network.slice     | pact    |
      | pact.slice/gpu.slice         | pact    |
      | pact.slice/audit.slice       | pact    |
      | workload.slice               | lattice |

  Scenario: pact-agent runs with OOM protection
    When pact-agent is running
    Then pact-agent should have OOMScoreAdj of -1000

  # --- Per-service cgroup scopes ---

  Scenario: Service start creates cgroup scope
    Given a service declaration for "chronyd" with memory limit "128M"
    When the service "chronyd" is started
    Then a cgroup scope should exist under "pact.slice/infra.slice" for "chronyd"
    And the scope memory.max should be set to "128M"

  Scenario: Service start fails if cgroup creation fails
    Given a service declaration for "broken-svc"
    And cgroup creation will fail for "broken-svc"
    When the service "broken-svc" is started
    Then the service "broken-svc" should be in state "Failed"
    And no orphaned cgroup scope should exist for "broken-svc"
    And an AuditEvent should be emitted for the cgroup creation failure

  Scenario: Process death triggers cgroup scope cleanup
    Given a running service "worker" in cgroup scope "pact.slice/infra.slice/worker"
    When the service "worker" crashes
    Then all processes in the cgroup scope should be killed via cgroup.kill
    And the cgroup scope for "worker" should be released

  Scenario: Forked children are killed on parent death
    Given a running service "multi-proc" that has forked 3 child processes
    When the main process of "multi-proc" dies
    Then all 3 child processes should be killed via cgroup.kill
    And the cgroup scope for "multi-proc" should be released
    And no orphaned processes should remain

  # --- Exclusive slice ownership ---

  Scenario: pact cannot write to workload slice in normal operation
    Given workload.slice exists and is owned by lattice
    When pact-agent attempts to create a scope in workload.slice
    Then the operation should be denied
    And no scope should be created in workload.slice

  Scenario: pact can read metrics from any slice
    Given workload.slice has active allocations
    When pact-agent reads memory.current from workload.slice
    Then the read should succeed
    And the metric value should be returned

  # --- Emergency override ---

  Scenario: Emergency mode allows killing workload slice processes
    Given an active emergency session on the node
    And workload.slice has running processes
    When pact-agent freezes workload.slice with "--force"
    Then all processes in workload.slice should be frozen
    And an AuditEvent should be emitted with action "EmergencyFreeze"
    And the AuditEvent should include the authenticated identity

  Scenario: Emergency kill in workload slice requires authentication
    Given workload.slice has running processes
    And no emergency session is active
    When pact-agent attempts to kill processes in workload.slice
    Then the operation should be denied
    And no processes should be affected

  # --- Namespace creation ---

  Scenario: Create namespace set for allocation
    When pact-agent creates namespaces for allocation "alloc-42"
    Then a pid namespace should be created
    And a net namespace should be created
    And a mount namespace should be created
    And a NamespaceSet should be tracked for "alloc-42"

  Scenario: Namespace cleanup on cgroup empty
    Given a NamespaceSet exists for allocation "alloc-42"
    And allocation "alloc-42" has an associated CgroupScope
    When all processes in the CgroupScope exit
    Then the NamespaceSet for "alloc-42" should be cleaned up
    And the CgroupScope should be released

  # --- Systemd backend ---

  Scenario: Systemd backend delegates cgroup management
    Given a supervisor with backend "systemd"
    When a service declaration for "chronyd" with memory limit "128M" is applied
    Then a systemd scope unit should be created with MemoryMax=128M
    And pact should not directly create cgroup entries
