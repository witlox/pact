Feature: Policy Evaluation
  pact-policy enforces OIDC authentication, RBAC authorization, and
  OPA/Rego policy evaluation. It runs as a library crate inside the
  journal process. (ADR-003)

  Background:
    Given a journal with default state

  # --- RBAC role enforcement ---

  Scenario: Platform admin has full access
    Given a user with role "pact-platform-admin"
    When the user requests to commit on vCluster "ml-training"
    Then the request should be authorized

  Scenario: Ops role can commit on their vCluster
    Given a user with role "pact-ops-ml-training"
    When the user requests to commit on vCluster "ml-training"
    Then the request should be authorized

  Scenario: Ops role cannot commit on other vCluster
    Given a user with role "pact-ops-ml-training"
    When the user requests to commit on vCluster "storage-ops"
    Then the request should be denied with reason "not authorized for vCluster storage-ops"

  Scenario: Viewer role can read status
    Given a user with role "pact-viewer-ml-training"
    When the user requests status for vCluster "ml-training"
    Then the request should be authorized

  Scenario: Viewer role cannot commit
    Given a user with role "pact-viewer-ml-training"
    When the user requests to commit on vCluster "ml-training"
    Then the request should be denied with reason "viewers cannot commit"

  Scenario: Service agent role for machine identity
    Given a user with role "pact-service-agent" and principal type "Service"
    When the agent authenticates to the journal
    Then the authentication should succeed

  Scenario: AI service role has limited write access
    Given a user with role "pact-service-ai" and principal type "Service"
    When the AI agent requests to read status
    Then the request should be authorized

  # --- Two-person approval (regulated vClusters) ---

  Scenario: Regulated vCluster requires two-person approval for commits
    Given vCluster "sensitive-data" has two-person approval enabled
    And a user with role "pact-regulated-sensitive-data"
    When the user requests to commit on vCluster "sensitive-data"
    Then the response should indicate approval required
    And a PendingApproval entry should be created in the journal

  Scenario: Second admin approves pending operation
    Given a pending approval for a commit on vCluster "sensitive-data"
    When a different admin with role "pact-regulated-sensitive-data" approves the operation
    Then the operation should proceed
    And the approval should be recorded in the journal

  Scenario: Same admin cannot approve their own operation
    Given a pending approval for a commit on vCluster "sensitive-data" by "admin@example.com"
    When "admin@example.com" tries to approve their own operation
    Then the approval should be rejected

  Scenario: Pending approval expires after timeout
    Given a pending approval for a commit on vCluster "sensitive-data"
    And the approval timeout is 30 minutes
    When 30 minutes have elapsed without approval
    Then the pending approval should be rejected
    And the rejection should be recorded in the journal

  # --- Policy evaluation via OPA ---

  Scenario: OPA evaluates complex policy rules
    Given OPA is running with pact authorization rules
    When a policy evaluation request is made for action "commit" on vCluster "ml-training"
    Then OPA should be called via localhost REST
    And the OPA decision should be returned

  Scenario: OPA unavailable falls back to cached policy
    Given OPA is unavailable
    And a cached VClusterPolicy exists for "ml-training"
    When a policy evaluation request is made
    Then the cached policy should be used for basic authorization
    And complex Rego rules should be skipped

  # --- Effective policy ---

  Scenario: Get effective policy for a vCluster
    Given vCluster "ml-training" has a policy with commit window 900 seconds
    When the effective policy for "ml-training" is requested
    Then the policy should include commit window 900
    And the policy should include the drift sensitivity
    And the policy should include the enforcement mode

  Scenario: Policy update replaces effective policy
    Given vCluster "ml-training" has a policy with commit window 900 seconds
    When the policy is updated to commit window 600 seconds
    Then the effective policy should reflect commit window 600

  # --- Action-specific authorization ---

  Scenario Outline: Action authorization matrix
    Given a user with role "<role>"
    When the user requests action "<action>" on vCluster "<vcluster>"
    Then the request should be <result>

    Examples:
      | role                          | action    | vcluster     | result     |
      | pact-platform-admin           | commit    | ml-training  | authorized |
      | pact-platform-admin           | emergency | ml-training  | authorized |
      | pact-ops-ml-training          | commit    | ml-training  | authorized |
      | pact-ops-ml-training          | exec      | ml-training  | authorized |
      | pact-ops-ml-training          | shell     | ml-training  | authorized |
      | pact-ops-ml-training          | emergency | ml-training  | authorized |
      | pact-viewer-ml-training       | status    | ml-training  | authorized |
      | pact-viewer-ml-training       | diff      | ml-training  | authorized |
      | pact-viewer-ml-training       | commit    | ml-training  | denied     |
      | pact-viewer-ml-training       | exec      | ml-training  | denied     |
      | pact-viewer-ml-training       | shell     | ml-training  | denied     |
      | pact-ops-ml-training          | commit    | storage-ops  | denied     |
