Feature: Node enrollment, domain membership, and certificate lifecycle
  Nodes must be enrolled in a pact domain before they can connect.
  Enrollment is a pre-registration step performed by a platform admin.
  The agent authenticates using hardware identity and a CSR — the journal
  signs the CSR locally with its intermediate CA key.

  Private keys never leave the agent. The journal stores only enrollment
  records and signed certificates (public data). No private key material
  exists in Raft state.

  vCluster assignment is independent of enrollment. An enrolled node
  can be unassigned (maintenance mode) or assigned to a vCluster.

  See ADR-008 for design rationale.

  Background:
    Given a pact domain "site-alpha" with a running journal quorum
    And journal nodes hold an intermediate CA signing key from Vault
    And the default certificate lifetime is 3 days

  # --- Node Enrollment (Admin) ---

  Scenario: Platform admin enrolls a node
    Given I am authenticated as "pact-platform-admin"
    When I run "pact node enroll compute-042 --mac aa:bb:cc:dd:ee:01 --bmc-serial SN12345"
    Then the enrollment should succeed
    And node "compute-042" should have enrollment state "Registered"

  Scenario: Batch enrollment of multiple nodes
    Given I am authenticated as "pact-platform-admin"
    And a CSV file "nodes.csv" with 100 node entries
    When I run "pact node enroll --batch nodes.csv"
    Then all 100 nodes should have enrollment state "Registered"

  Scenario: Batch enrollment with partial failure
    Given I am authenticated as "pact-platform-admin"
    And a CSV file with 10 nodes where 3 have duplicate MACs of existing enrollments
    When I run "pact node enroll --batch nodes.csv"
    Then 7 nodes should succeed with state "Registered"
    And 3 nodes should fail with "NODE_ALREADY_ENROLLED" or "HARDWARE_IDENTITY_CONFLICT"
    And the response should include per-node status

  Scenario: Non-admin cannot enroll nodes
    Given I am authenticated as "pact-ops-ml-training"
    When I run "pact node enroll compute-042 --mac aa:bb:cc:dd:ee:01"
    Then the command should fail with "PERMISSION_DENIED"
    And no enrollment record should exist for "compute-042"

  Scenario: Duplicate enrollment is rejected
    Given node "compute-042" is already enrolled with mac "aa:bb:cc:dd:ee:01"
    And I am authenticated as "pact-platform-admin"
    When I run "pact node enroll compute-042 --mac aa:bb:cc:dd:ee:01"
    Then the command should fail with "NODE_ALREADY_ENROLLED"

  Scenario: Enrollment with duplicate hardware identity is rejected
    Given node "compute-042" is enrolled with mac "aa:bb:cc:dd:ee:01"
    And I am authenticated as "pact-platform-admin"
    When I run "pact node enroll compute-099 --mac aa:bb:cc:dd:ee:01"
    Then the command should fail with "HARDWARE_IDENTITY_CONFLICT"

  # --- Boot Enrollment (Agent CSR) ---

  Scenario: Agent enrolls on first boot with CSR
    Given node "compute-042" is enrolled with mac "aa:bb:cc:dd:ee:01" and bmc-serial "SN12345"
    When agent boots with hardware identity mac "aa:bb:cc:dd:ee:01" and bmc-serial "SN12345"
    And agent generates a keypair and CSR
    And agent calls Enroll with hardware identity and CSR on the journal
    Then the journal should sign the CSR with its intermediate CA key
    And return the signed certificate and node "compute-042" identity
    And node "compute-042" should have enrollment state "Active"
    And the agent should establish an mTLS connection using its own private key and the signed cert

  Scenario: Agent private key never leaves the agent
    When agent generates a keypair and CSR for enrollment
    Then the private key should exist only in agent memory
    And the CSR should contain only the public key
    And the journal should never receive or store the private key

  Scenario: Agent with unknown hardware identity is rejected
    When agent boots with hardware identity mac "ff:ff:ff:ff:ff:ff" and bmc-serial "UNKNOWN"
    And agent generates a keypair and CSR
    And agent calls Enroll on the journal
    Then the journal should reject with "NODE_NOT_ENROLLED"
    And no certificate should be signed

  Scenario: Agent with revoked enrollment is rejected
    Given node "compute-042" was enrolled but has been decommissioned
    When agent boots with hardware identity mac "aa:bb:cc:dd:ee:01" and bmc-serial "SN12345"
    And agent calls Enroll on the journal
    Then the journal should reject with "NODE_REVOKED"

  Scenario: Active node rejects duplicate enrollment
    Given node "compute-042" is enrolled and currently "Active"
    When a second agent calls Enroll with matching hardware identity
    Then the journal should reject with "ALREADY_ACTIVE"
    And the existing active node's certificate should not be affected

  Scenario: Agent re-enrolls after being inactive
    Given node "compute-042" is enrolled and was previously active
    And node "compute-042" is currently in state "Inactive" (heartbeat timeout)
    When agent boots with hardware identity mac "aa:bb:cc:dd:ee:01" and bmc-serial "SN12345"
    And agent generates a new keypair and CSR
    And agent calls Enroll on the journal
    Then the journal should sign the new CSR
    And node "compute-042" should have enrollment state "Active"

  # --- Enrollment Response includes vCluster ---

  Scenario: Enrollment response includes vCluster assignment
    Given node "compute-042" is enrolled and assigned to vCluster "ml-training"
    And node "compute-042" is in state "Inactive"
    When agent boots and successfully enrolls
    Then the enrollment response should include vcluster_id "ml-training"
    And the agent should immediately stream boot config for "ml-training"

  Scenario: Enrollment response indicates maintenance mode
    Given node "compute-042" is enrolled with no vCluster assignment
    When agent boots and successfully enrolls
    Then the enrollment response should include vcluster_id "none"
    And the agent should enter maintenance mode

  # --- Enrollment Endpoint Security ---

  Scenario: Enrollment endpoint uses server-TLS-only
    When an agent connects to the enrollment endpoint
    Then the connection should use TLS (server cert validated)
    And the server should NOT require a client certificate

  Scenario: Enrollment endpoint is rate-limited
    When more than 100 enrollment requests arrive within 1 minute
    Then requests beyond the limit should be rejected with "RATE_LIMITED"
    And a warning should be logged

  Scenario: All enrollment attempts are audit-logged
    When an agent calls Enroll with unknown hardware identity
    Then the failed enrollment attempt should be logged to the audit trail
    And forwarded to Loki with the source IP and presented hardware identity

  Scenario: Authenticated endpoints reject unauthenticated access
    When an unauthenticated client calls ConfigService.AppendEntry
    Then the request should be rejected with "UNAUTHENTICATED"
    When an unauthenticated client calls EnrollmentService.RegisterNode
    Then the request should be rejected with "UNAUTHENTICATED"

  # --- Heartbeat (Subscription Stream Liveness) ---

  Scenario: Active node detected as inactive on stream disconnect
    Given node "compute-042" is "Active" with a config subscription stream
    And the heartbeat timeout is 5 minutes
    When the subscription stream disconnects
    And 5 minutes elapse without reconnection
    Then node "compute-042" should transition to "Inactive"
    And the transition should be recorded as a Raft write

  Scenario: Reconnection within grace period preserves Active state
    Given node "compute-042" is "Active" with a config subscription stream
    And the heartbeat timeout is 5 minutes
    When the subscription stream disconnects
    And the agent reconnects within 3 minutes
    Then node "compute-042" should remain "Active"

  # --- Boot Storm ---

  Scenario: 1000 concurrent boot enrollments signed locally
    Given 1000 nodes are enrolled in state "Registered"
    When 1000 agents simultaneously call Enroll with CSRs
    Then all 1000 CSRs should be signed by the journal's intermediate CA
    And no requests should be made to Vault during enrollment
    And all 1000 agents should establish mTLS connections

  # --- Certificate Rotation ---

  Scenario: Agent renews certificate with new CSR
    Given node "compute-042" is active with a certificate expiring in 24 hours
    When the agent generates a new keypair and CSR
    And calls RenewCert with the current cert serial and new CSR
    Then the journal should sign the new CSR locally
    And return the new signed certificate

  Scenario: Dual-channel rotation does not interrupt operations
    Given node "compute-042" is active with an active mTLS channel
    And a shell session is open on node "compute-042"
    When certificate renewal triggers
    Then the agent should generate a new keypair and CSR
    And obtain a new signed certificate from the journal
    And build a passive channel with the new key and cert
    And health-check the passive channel
    And swap: passive becomes active, old active drains
    And the shell session should continue uninterrupted

  Scenario: Failed renewal does not disrupt active channel
    Given node "compute-042" is active with a certificate expiring in 24 hours
    And the journal is temporarily unreachable
    When the agent attempts certificate renewal
    Then the renewal should fail
    And the active channel should continue functioning
    And the agent should log a warning about upcoming certificate expiry
    And the agent should retry renewal on the next interval

  Scenario: Certificate expires without successful renewal
    Given node "compute-042" is active with an expired certificate
    And all renewal attempts have failed
    Then the agent should enter degraded mode
    And use cached configuration (invariant A9)
    And continue retrying enrollment
    When the journal becomes reachable
    Then the agent should re-enroll with a new CSR
    And establish a new mTLS connection

  # --- vCluster Assignment (independent of enrollment) ---

  Scenario: Assign enrolled node to a vCluster
    Given node "compute-042" is enrolled and active with no vCluster assignment
    And I am authenticated as "pact-platform-admin"
    When I run "pact node assign compute-042 --vcluster ml-training"
    Then node "compute-042" should be assigned to vCluster "ml-training"
    And the agent should receive the "ml-training" boot overlay
    And drift detection should activate with "ml-training" policy

  Scenario: Ops role can assign nodes to their vCluster
    Given node "compute-042" is enrolled and active with no vCluster assignment
    And I am authenticated as "pact-ops-ml-training"
    When I run "pact node assign compute-042 --vcluster ml-training"
    Then node "compute-042" should be assigned to vCluster "ml-training"

  Scenario: Ops role cannot assign nodes to other vClusters
    Given node "compute-042" is enrolled and active with no vCluster assignment
    And I am authenticated as "pact-ops-ml-training"
    When I run "pact node assign compute-042 --vcluster regulated-bio"
    Then the command should fail with "PERMISSION_DENIED"

  Scenario: Unassign node from vCluster (maintenance mode)
    Given node "compute-042" is assigned to vCluster "ml-training"
    And I am authenticated as "pact-platform-admin"
    When I run "pact node unassign compute-042"
    Then node "compute-042" should have no vCluster assignment
    And the agent should apply domain defaults only
    And drift detection should be disabled
    And the node should not be schedulable by lattice

  Scenario: Move node between vClusters
    Given node "compute-042" is assigned to vCluster "ml-training"
    And I am authenticated as "pact-platform-admin"
    When I run "pact node move compute-042 --to-vcluster regulated-bio"
    Then node "compute-042" should be assigned to vCluster "regulated-bio"
    And the agent should receive the "regulated-bio" boot overlay
    And the "ml-training" policy should no longer apply

  Scenario: Moving a node does not affect certificate
    Given node "compute-042" is assigned to vCluster "ml-training"
    And has an mTLS certificate with identity "pact-service-agent/compute-042@site-alpha"
    When I run "pact node move compute-042 --to-vcluster regulated-bio"
    Then the mTLS certificate should remain unchanged
    And the mTLS connection should not be interrupted

  # --- Maintenance Mode (Active + Unassigned) ---

  Scenario: Unassigned node runs in maintenance mode
    Given node "compute-042" is enrolled and active with no vCluster assignment
    Then the agent should apply domain-default configuration only
    And the agent should run time sync service if configured in domain defaults
    And no workload services should be started
    And platform admin should be able to exec on the node
    And no vCluster-scoped roles should be active
    And capability report should have vcluster "none"

  Scenario: Unassigned node is not schedulable
    Given node "compute-042" is active with no vCluster assignment
    Then the capability report should indicate vcluster "none"
    And lattice scheduler should not schedule jobs on this node

  # --- Node Decommissioning ---

  Scenario: Decommission a node with no active sessions
    Given node "compute-042" is enrolled and active with no active sessions
    And I am authenticated as "pact-platform-admin"
    When I run "pact node decommission compute-042"
    Then node "compute-042" should have enrollment state "Revoked"
    And the certificate serial should be published to Vault CRL
    And the agent's mTLS connection should be terminated

  Scenario: Decommission warns on active sessions
    Given node "compute-042" is enrolled and active
    And an admin has an active shell session on node "compute-042"
    And I am authenticated as "pact-platform-admin"
    When I run "pact node decommission compute-042"
    Then the command should warn "1 active session(s) on this node"
    And ask for confirmation or suggest "--force"

  Scenario: Decommission with --force terminates sessions
    Given node "compute-042" has active shell sessions
    And I am authenticated as "pact-platform-admin"
    When I run "pact node decommission compute-042 --force"
    Then the active sessions should be terminated
    And session audit records should be preserved
    And node "compute-042" should have enrollment state "Revoked"

  Scenario: Decommissioned node cannot re-enroll without new enrollment
    Given node "compute-042" has been decommissioned
    When agent boots with matching hardware identity and calls Enroll
    Then the journal should reject with "NODE_REVOKED"

  Scenario: Non-admin cannot decommission nodes
    Given node "compute-042" is enrolled and active
    And I am authenticated as "pact-ops-ml-training"
    When I run "pact node decommission compute-042"
    Then the command should fail with "PERMISSION_DENIED"

  # --- Multi-Domain Enrollment (Shared Hardware) ---

  Scenario: Node enrolled in two domains is active in only one
    Given node "special-gpu-001" is enrolled in domain "site-alpha"
    And node "special-gpu-001" is also enrolled in domain "site-beta"
    When node boots into domain "site-alpha" via Manta-alpha
    Then node should be "Active" in domain "site-alpha"
    And node should remain "Registered" in domain "site-beta"

  Scenario: Node moves between domains via reboot
    Given node "special-gpu-001" is "Active" in domain "site-alpha"
    When node is rebooted into domain "site-beta" via Manta-beta
    Then domain "site-alpha" should detect subscription stream disconnect
    And after heartbeat timeout node should become "Inactive" in domain "site-alpha"
    And node should become "Active" in domain "site-beta"
    And node should receive a new certificate signed by domain "site-beta" journal

  Scenario: Inactive domain does not block activation in other domain
    Given node "special-gpu-001" is "Inactive" in domain "site-alpha"
    When node boots into domain "site-beta"
    Then enrollment in domain "site-beta" should succeed
    And no coordination with domain "site-alpha" is required

  # --- Sovra Cross-Domain Visibility (Optional) ---

  Scenario: Sovra publishes enrollment claim on activation
    Given Sovra federation is configured
    And node "special-gpu-001" is enrolled in domains "site-alpha" and "site-beta"
    When node becomes "Active" in domain "site-alpha"
    Then domain "site-alpha" should publish an enrollment claim via Sovra
    And domain "site-beta" should see that the node is active elsewhere

  Scenario: Sovra warns on concurrent active enrollment
    Given Sovra federation is configured
    And Sovra reports node "special-gpu-001" is active in domain "site-alpha"
    And I am authenticated as "pact-platform-admin" in domain "site-beta"
    When node "special-gpu-001" attempts to enroll in domain "site-beta"
    Then the journal should log a warning "node active in another domain"
    And enrollment should still succeed (advisory, not blocking)

  Scenario: Sovra unavailable does not block enrollment
    Given Sovra federation is configured but unreachable
    When node "special-gpu-001" boots and enrolls in domain "site-beta"
    Then enrollment should succeed
    And the journal should log "Sovra unreachable — cross-domain visibility unavailable"

  # --- Inventory Queries ---

  Scenario: List enrolled nodes
    Given nodes "compute-001" through "compute-010" are enrolled
    And I am authenticated as "pact-platform-admin"
    When I run "pact node list"
    Then I should see all 10 nodes with their enrollment state and vCluster assignment

  Scenario: List nodes by state
    Given 5 nodes are "Active", 3 are "Inactive", 2 are "Registered"
    When I run "pact node list --state active"
    Then I should see only the 5 active nodes

  Scenario: List unassigned nodes
    Given 3 nodes are enrolled but not assigned to any vCluster
    When I run "pact node list --unassigned"
    Then I should see only the 3 unassigned nodes

  Scenario: Inspect node details
    Given node "compute-042" is active and assigned to "ml-training"
    When I run "pact node inspect compute-042"
    Then I should see the enrollment state "Active"
    And the hardware identity (mac, bmc-serial)
    And the vCluster assignment "ml-training"
    And the certificate serial and expiry
    And the last seen timestamp

  Scenario: Viewer can list and inspect nodes in their vCluster
    Given node "compute-042" is assigned to "ml-training"
    And I am authenticated as "pact-viewer-ml-training"
    When I run "pact node list --vcluster ml-training"
    Then I should see "compute-042"
    When I run "pact node inspect compute-042"
    Then I should see the node details

  Scenario: Viewer cannot see nodes in other vClusters
    Given node "compute-042" is assigned to "regulated-bio"
    And I am authenticated as "pact-viewer-ml-training"
    When I run "pact node inspect compute-042"
    Then the command should fail with "PERMISSION_DENIED"
