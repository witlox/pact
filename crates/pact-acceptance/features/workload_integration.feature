Feature: Workload Integration
  Integration protocol between pact-agent and lattice-node-agent for namespace
  handoff, mount sharing, and boot readiness. Defined in hpc-core (hpc-node)
  as a shared contract. (Invariants: WI1-WI6)

  Background:
    Given a supervisor with backend "pact"
    And lattice-node-agent is a supervised service

  # --- Namespace handoff ---

  Scenario: Namespace handoff via unix socket
    When pact-agent creates namespaces for allocation "alloc-42"
    Then namespace FDs should be available for handoff
    And the handoff should use the unix socket at the hpc-node defined path
    And SCM_RIGHTS should be used for FD passing

  Scenario: lattice receives namespace FDs
    Given pact-agent created namespaces for allocation "alloc-42"
    When lattice-node-agent connects to the handoff socket
    And requests namespaces for "alloc-42"
    Then pid, net, and mount namespace FDs should be received
    And lattice can spawn workload processes inside those namespaces

  Scenario: Namespace handoff failure falls back to self-service
    Given the handoff unix socket is unavailable
    When lattice-node-agent needs namespaces for allocation "alloc-42"
    Then lattice should create its own namespaces (self-service mode)
    And an AuditEvent should be emitted noting the fallback
    And the allocation should proceed with reduced isolation guarantees

  # --- Mount refcounting ---

  Scenario: First allocation mounts uenv image
    Given no mount exists for uenv image "pytorch-2.5.sqfs"
    When allocation "alloc-01" requests uenv "pytorch-2.5.sqfs"
    Then the SquashFS image should be mounted once
    And a MountRef should be created with refcount 1
    And a bind-mount should be placed in alloc-01's mount namespace

  Scenario: Second allocation shares existing mount
    Given "pytorch-2.5.sqfs" is mounted with refcount 1 for alloc-01
    When allocation "alloc-02" requests uenv "pytorch-2.5.sqfs"
    Then no new SquashFS mount should occur
    And MountRef refcount should increase to 2
    And a bind-mount should be placed in alloc-02's mount namespace

  Scenario: Allocation release decrements refcount
    Given "pytorch-2.5.sqfs" is mounted with refcount 2
    When allocation "alloc-01" releases
    Then MountRef refcount should decrease to 1
    And the SquashFS mount should remain

  Scenario: Last allocation release starts hold timer
    Given "pytorch-2.5.sqfs" is mounted with refcount 1
    When the last allocation releases
    Then MountRef refcount should be 0
    And a cache hold timer should start
    And the mount should not be unmounted yet

  Scenario: Hold timer expiry unmounts image
    Given "pytorch-2.5.sqfs" has refcount 0 and hold timer running
    When the hold timer expires
    Then the SquashFS image should be unmounted
    And the MountRef should be removed

  Scenario: New allocation during hold timer reuses mount
    Given "pytorch-2.5.sqfs" has refcount 0 and hold timer running
    When allocation "alloc-03" requests uenv "pytorch-2.5.sqfs"
    Then the hold timer should be cancelled
    And MountRef refcount should increase to 1
    And no new SquashFS mount should occur

  Scenario: Emergency force-unmount overrides hold timer
    Given "pytorch-2.5.sqfs" has refcount 0 and hold timer running
    And an active emergency session with "--force"
    When emergency unmount is requested
    Then the SquashFS image should be unmounted immediately
    And the hold timer should be cancelled
    And an AuditEvent should be emitted for the force-unmount

  # --- Namespace cleanup ---

  Scenario: Namespace cleanup when cgroup empties
    Given allocation "alloc-42" has a NamespaceSet and CgroupScope
    And 3 processes are running in the CgroupScope
    When all 3 processes exit
    Then the CgroupScope should become empty
    And pact should detect the empty cgroup
    And the NamespaceSet for "alloc-42" should be cleaned up
    And associated bind-mounts should be released

  Scenario: Namespace cleanup resilient to lattice crash
    Given allocation "alloc-42" has a NamespaceSet and CgroupScope
    And lattice-node-agent crashes
    When all processes in alloc-42's CgroupScope eventually exit
    Then pact should still detect the empty cgroup
    And the NamespaceSet should be cleaned up
    And no manual intervention should be required

  # --- Mount refcount reconstruction ---

  Scenario: Agent restart reconstructs refcounts
    Given pact-agent is running with 2 active allocations:
      | allocation | uenv             |
      | alloc-01   | pytorch-2.5.sqfs |
      | alloc-02   | pytorch-2.5.sqfs |
    And MountRef for "pytorch-2.5.sqfs" has refcount 2
    When pact-agent crashes and restarts
    Then pact-agent should scan the kernel mount table
    And correlate mounts with active allocations from journal state
    And reconstruct MountRef for "pytorch-2.5.sqfs" with refcount 2
    And no mounts should be disrupted

  Scenario: Agent restart detects orphaned mounts
    Given pact-agent crashes
    And allocation "alloc-01" ended while agent was down
    When pact-agent restarts and reconstructs state
    Then the mount for alloc-01's uenv should have refcount 0
    And a hold timer should start for the orphaned mount

  # --- Readiness gate ---

  Scenario: Readiness gate signals lattice
    When pact-agent completes all boot phases and emits ReadinessSignal
    Then the readiness gate should be open
    And lattice-node-agent should be able to request namespaces and mounts

  Scenario: Lattice requests before readiness are queued
    Given pact-agent is still in StartServices boot phase
    When lattice-node-agent requests namespaces for allocation "alloc-01"
    Then the request should be queued until ReadinessSignal is emitted
    And the allocation should not be rejected

  # --- Standalone lattice ---

  Scenario: Lattice standalone creates own hierarchy
    Given lattice-node-agent runs without pact (standalone mode)
    When lattice-node-agent starts
    Then lattice should create workload.slice/ using hpc-node conventions
    And lattice should manage its own mounts and namespaces
    And no unix socket handoff should be attempted
