Feature: RBAC Authorization
  Every pact operation is authenticated via OIDC and authorized via RBAC.
  Roles are scoped per vCluster with 6 role types.

  Background:
    Given a journal with default state
    And the following vClusters exist:
      | name          |
      | ml-training   |
      | storage-ops   |
      | regulated-hpc |

  # --- Role definitions ---

  Scenario: Platform admin has access to all vClusters
    Given a user "admin@example.com" with role "pact-platform-admin"
    When the user queries status for vCluster "ml-training"
    Then the request should be authorized
    When the user queries status for vCluster "storage-ops"
    Then the request should be authorized

  Scenario: Ops role scoped to single vCluster
    Given a user "ops@example.com" with role "pact-ops-ml-training"
    When the user queries status for vCluster "ml-training"
    Then the request should be authorized
    When the user queries status for vCluster "storage-ops"
    Then the request should be denied

  Scenario: Viewer role is read-only
    Given a user "viewer@example.com" with role "pact-viewer-ml-training"
    When the user requests to view diff for vCluster "ml-training"
    Then the request should be authorized
    When the user requests to commit on vCluster "ml-training"
    Then the request should be denied

  Scenario: Regulated role requires two-person approval
    Given a user "regulated@example.com" with role "pact-regulated-regulated-hpc"
    And vCluster "regulated-hpc" has two-person approval enabled
    When the user requests to commit on vCluster "regulated-hpc"
    Then the response should require approval from a second administrator

  # --- Machine identities ---

  Scenario: Service agent authenticates with machine identity
    Given a service with role "pact-service-agent" and principal type "Service"
    When the service authenticates
    Then the authentication should succeed
    And the principal type should be "Service"

  Scenario: AI agent has restricted permissions
    Given a service with role "pact-service-ai" and principal type "Service"
    When the AI agent requests to enter emergency mode
    Then the request should be denied
    When the AI agent requests to read fleet status
    Then the request should be authorized

  # --- OIDC integration ---

  Scenario: Valid OIDC token is accepted
    Given a valid OIDC token for "admin@example.com" with groups "pact-ops-ml-training"
    When the token is presented for authentication
    Then the principal should be extracted as "admin@example.com"
    And the role should be mapped to "pact-ops-ml-training"

  Scenario: Expired OIDC token is rejected
    Given an expired OIDC token for "admin@example.com"
    When the token is presented for authentication
    Then the authentication should fail with "token expired"

  Scenario: Token with wrong audience is rejected
    Given an OIDC token with wrong audience
    When the token is presented for authentication
    Then the authentication should fail with "invalid audience"

  # --- Per-operation authorization ---

  Scenario: Operations are scoped by vCluster
    Given a user "ops@example.com" with role "pact-ops-ml-training"
    Then the following operations should be authorized for vCluster "ml-training":
      | operation  |
      | commit     |
      | rollback   |
      | exec       |
      | shell      |
      | service    |
      | emergency  |
      | status     |
      | diff       |
      | log        |
    And the following operations should be denied for vCluster "storage-ops":
      | operation  |
      | commit     |
      | rollback   |
      | exec       |
      | shell      |
      | service    |
      | emergency  |
