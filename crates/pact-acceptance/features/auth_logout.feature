Feature: Authentication Logout (hpc-auth crate)

  Scenario: Logout with IdP reachable
    Given a valid token exists in the cache
    And the IdP is reachable
    When the user initiates logout
    Then the system revokes the refresh token at the IdP
    And deletes the cached tokens for the current server

  Scenario: Logout with IdP unreachable
    Given a valid token exists in the cache
    And the IdP is unreachable
    When the user initiates logout
    Then the system attempts to revoke the refresh token
    And deletes the cached tokens regardless of revocation result

  Scenario: Logout when not logged in
    Given no token exists in the cache for the current server
    When the user initiates logout
    Then the system informs the user they are not logged in
