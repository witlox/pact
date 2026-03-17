Feature: Identity Mapping
  OIDC-to-POSIX UID/GID translation for NFS compatibility. This is a bypass
  shim — only active when pact is init (PactSupervisor mode) AND NFS storage
  is used. Not a core identity system. (Invariants: IM1-IM7)

  # --- Activation conditions ---

  Scenario: Identity mapping active in PactSupervisor mode
    Given a supervisor with backend "pact"
    And NFS storage is configured
    When pact-agent boots
    Then /run/pact/passwd.db should be created
    And /run/pact/group.db should be created
    And /etc/nsswitch.conf should include "pact" for passwd and group

  Scenario: Identity mapping inactive in systemd mode
    Given a supervisor with backend "systemd"
    When pact-agent starts
    Then /run/pact/passwd.db should not exist
    And /run/pact/group.db should not exist

  # --- On-demand UID assignment ---

  Scenario: First authentication assigns UID from org range
    Given identity_mode is "on-demand"
    And org "local" has org_index 0 with stride 10000 and base_uid 10000
    And no UidEntry exists for subject "newuser@cscs.ch"
    When "newuser@cscs.ch" authenticates via OIDC
    Then a UidEntry should be created with uid in range 10000-19999
    And the assignment should be committed to the journal via Raft
    And the .db files should be updated on all agents

  Scenario: Sequential UID assignment within precursor range
    Given org "local" has org_index 0 with stride 10000 and base_uid 10000
    And UIDs 10000 through 10004 are already assigned
    When a new subject "user5@cscs.ch" is assigned a UID
    Then the assigned UID should be 10005

  Scenario: UID range exhaustion fails with error
    Given org "local" has stride 3 and all 3 UIDs are assigned
    When a new subject "overflow@cscs.ch" is assigned a UID
    Then the assignment should fail with "UID range exhausted"
    And an alert should be emitted

  # --- Pre-provisioned mode ---

  Scenario: Pre-provisioned mode rejects unknown subjects
    Given identity_mode is "pre-provisioned"
    And no UidEntry exists for subject "unknown@cscs.ch"
    When "unknown@cscs.ch" authenticates via OIDC
    Then the authentication should succeed (OIDC is valid)
    But operations requiring UID should be rejected with "identity not provisioned"

  Scenario: Pre-provisioned mode accepts known subjects
    Given identity_mode is "pre-provisioned"
    And a UidEntry exists for "known@cscs.ch" with uid 10042
    When "known@cscs.ch" authenticates and accesses NFS
    Then UID 10042 should be used for file ownership

  # --- UID stability ---

  Scenario: UID assignment is stable across reboots
    Given "user@cscs.ch" was assigned UID 10003
    When pact-agent reboots and reloads UidMap from journal
    Then "user@cscs.ch" should still have UID 10003

  Scenario: UID assignment is consistent across nodes
    Given "user@cscs.ch" was assigned UID 10003 on node "node-001"
    When node "node-002" receives the UidMap via journal subscription
    Then "user@cscs.ch" should have UID 10003 on node "node-002"

  # --- Federation ---

  Scenario: Federated org gets computed precursor range
    Given base_uid is 10000 and stride is 10000
    When org "partner-a" joins federation with org_index 2
    Then partner-a's precursor should be 30000
    And partner-a's UID range should be 30000-39999

  Scenario: Federated orgs have non-overlapping ranges
    Given org "local" with org_index 0 (range 10000-19999)
    And org "partner-a" with org_index 1 (range 20000-29999)
    When "userA@partner-a.org" is assigned UID 20000
    And "userB@local" is assigned UID 10005
    Then no UID collision exists

  Scenario: Federation departure triggers GC
    Given org "partner-a" with org_index 1 has 50 assigned UidEntries
    When org "partner-a" leaves federation
    Then all 50 UidEntries for "partner-a" should be removed from journal
    And org_index 1 should be reclaimable
    And NFS files owned by those UIDs become orphaned

  # --- Group resolution ---

  Scenario: Full supplementary group resolution
    Given "user@cscs.ch" is a member of groups:
      | group    | gid  |
      | lp16     | 2001 |
      | csstaff  | 3000 |
      | gpu-users| 3050 |
    When getgrouplist("user") is called via NSS
    Then groups lp16, csstaff, and gpu-users should all be returned
    And GIDs 2001, 3000, 3050 should be included

  # --- NSS module behavior ---

  Scenario: NSS module reads from local files only
    Given /run/pact/passwd.db contains entry for "user@cscs.ch" with uid 10003
    When getpwuid(10003) is called
    Then the result should be returned from local file
    And no network call should be made

  Scenario: NSS module handles missing .db files gracefully
    Given /run/pact/passwd.db does not exist
    When getpwnam("user") is called via pact NSS module
    Then the lookup should return not found
    And nsswitch should fall through to the next source

  # --- Runtime UidMap updates ---

  Scenario: New user assignment propagates to running agents
    Given pact-agent is running on node "node-001"
    And "newuser@cscs.ch" is assigned UID 10010 on the journal
    When the journal subscription delivers the UidMap update
    Then /run/pact/passwd.db should be updated
    And getpwnam("newuser") should now resolve to UID 10010

  Scenario: Non-root service waits for UidMap
    Given a service "app-svc" declared with user "appuser"
    And UidMap is not yet loaded
    When the service "app-svc" startup is attempted
    Then startup should wait for UidMap to be available
    And "app-svc" should start only after "appuser" resolves to a UID
