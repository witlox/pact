Feature: CLI Authentication (PACT-specific)

  PACT CLI wraps the shared hpc-auth crate for token acquisition.
  These scenarios cover PACT-specific behavior on top of the crate.
  PACT uses strict permission mode and integrates with emergency mode
  and policy evaluation.

  # --- CLI Commands ---

  Scenario: pact login initiates auth flow
    Given the pact-journal server is configured
    When the user runs pact login
    Then the system discovers the IdP from the journal auth discovery endpoint
    And delegates to the hpc-auth crate for token acquisition
    And uses strict permission mode for the token cache

  Scenario: pact login with explicit server
    Given no default server is configured
    When the user runs pact login --server journal.example.com
    Then the system contacts journal.example.com for IdP discovery
    And sets journal.example.com as the default server

  Scenario: pact logout clears token
    Given the user is logged in
    When the user runs pact logout
    Then the system delegates to the hpc-auth crate for logout
    And the user is informed the session has ended

  Scenario: pact login with --device-code
    Given the pact-journal server is configured
    When the user runs pact login --device-code
    Then the system forces the device code flow via the hpc-auth crate

  Scenario: pact login with --service-account
    Given client_id and client_secret are available via config or environment
    When the user runs pact login --service-account
    Then the system delegates to the hpc-auth crate client credentials flow

  # --- Unauthenticated Commands ---

  Scenario: pact version does not require auth
    Given no token exists in the cache
    When the user runs pact version
    Then the command succeeds and displays the version

  Scenario: pact --help does not require auth
    Given no token exists in the cache
    When the user runs pact --help
    Then the command succeeds and displays help text

  # --- Authenticated Commands ---

  Scenario: pact status requires auth
    Given no token exists in the cache
    When the user runs pact status
    Then the command fails with an authentication error
    And the user is prompted to run pact login

  Scenario: pact exec requires auth
    Given no token exists in the cache
    When the user runs pact exec node-001 -- uptime
    Then the command fails with an authentication error
    And the user is prompted to run pact login

  Scenario: pact shell requires auth
    Given no token exists in the cache
    When the user runs pact shell node-001
    Then the command fails with an authentication error
    And the user is prompted to run pact login

  Scenario: Authenticated command with valid token
    Given a valid token exists in the cache for the configured server
    When the user runs pact status
    Then the token is included in the request to pact-journal
    And the command proceeds

  Scenario: Authenticated command triggers silent refresh
    Given an expired access token in the cache
    And a valid refresh token in the cache
    When the user runs pact status
    Then the hpc-auth crate silently refreshes the access token
    And the command proceeds with the new token

  # --- Strict Permission Mode ---

  Scenario: Cache with wrong permissions rejected in strict mode
    Given a token cache file with permissions 0644
    When the user runs pact status
    Then the command fails with a security error
    And the error explains that cache permissions must be 0600
    And the user is prompted to run pact login again

  Scenario: Cache with correct permissions accepted
    Given a token cache file with permissions 0600
    And the cached token is valid
    When the user runs pact status
    Then the command proceeds normally

  # --- RBAC Integration ---

  Scenario: Valid token but insufficient RBAC role for operation
    Given the user is authenticated
    And the user has role "pact-viewer-ml-training"
    When the user runs pact commit -m "change" on vCluster "ml-training"
    Then the command fails with an authorization error
    And the error explains the user lacks commit permissions

  Scenario: Valid token with correct RBAC role
    Given the user is authenticated
    And the user has role "pact-ops-ml-training"
    When the user runs pact commit -m "change" on vCluster "ml-training"
    Then the command is accepted

  Scenario: Platform admin authorized for all vClusters
    Given the user is authenticated
    And the user has role "pact-platform-admin"
    When the user runs pact status on any vCluster
    Then the command is accepted

  # --- Emergency Mode Integration ---

  Scenario: Emergency mode does not bypass authentication
    Given no token exists in the cache
    When the user runs pact emergency --start on node-001
    Then the command fails with an authentication error
    And the user is prompted to run pact login

  Scenario: Emergency mode requires human admin token
    Given the user is authenticated
    And the user has principal type "Service" (pact-service-ai)
    When the user runs pact emergency --start on node-001
    Then the command fails with an authorization error
    And the error explains that emergency mode requires a human admin

  Scenario: Emergency mode with valid human admin token
    Given the user is authenticated
    And the user has role "pact-ops-ml-training"
    And node-001 belongs to vCluster "ml-training"
    When the user runs pact emergency --start on node-001
    Then the command proceeds

  # --- Two-Person Approval ---

  Scenario: Regulated operation requires two-person approval
    Given the user is authenticated
    And the user has role "pact-regulated-regulated-hpc"
    And vCluster "regulated-hpc" has two-person approval enabled
    When the user runs pact commit -m "change" on vCluster "regulated-hpc"
    Then the operation enters pending approval state
    And a second admin must approve before the commit proceeds

  Scenario: User cannot approve their own pending operation
    Given the user is authenticated as "admin-a@example.com"
    And admin-a has a pending approval for a commit on "regulated-hpc"
    When admin-a attempts to approve their own pending operation
    Then the approval is rejected
    And the error explains that self-approval is not permitted

  # --- Journal Discovery Endpoint ---

  Scenario: pact-journal exposes auth discovery
    Given the pact-journal server is running
    When a client requests the auth discovery endpoint
    Then the server returns the IdP URL and public client ID
    And the endpoint does not require authentication

  # --- Multi-Server ---

  Scenario: Switching between pact deployments
    Given the user is logged in to journal-a as default
    And the user is logged in to journal-b
    When the user runs pact status --server journal-b
    Then the command uses the token cached for journal-b

  Scenario: Default server used when --server omitted
    Given the user is logged in to journal-a as default
    And the user is logged in to journal-b
    When the user runs pact status
    Then the command uses the token cached for journal-a

  # --- Break Glass ---

  Scenario: Emergency access when IdP is down
    Given the IdP is unreachable
    And the user's cached tokens have expired
    When the user attempts pact login
    Then pact login fails with IdP unreachable error
    And the error suggests using pact emergency via BMC console as break-glass
