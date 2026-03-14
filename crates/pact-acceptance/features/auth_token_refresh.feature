Feature: Token Refresh and Cache (hpc-auth crate)

  # --- Silent Refresh ---

  Scenario: Access token expired, refresh token valid
    Given an expired access token in the cache
    And a valid refresh token in the cache
    When any authenticated command is executed
    Then the system silently refreshes the access token
    And updates the cache
    And the command proceeds with the new access token

  Scenario: Both tokens expired
    Given an expired access token in the cache
    And an expired refresh token in the cache
    When any authenticated command is executed
    Then the command fails with an authentication error
    And the user is prompted to run login again

  Scenario: No refresh token available, access token expired
    Given an expired access token in the cache
    And no refresh token in the cache
    When any authenticated command is executed
    Then the command fails with an authentication error
    And the user is prompted to run login again

  Scenario: Refresh returns reduced scopes
    Given an expired access token in the cache
    And a valid refresh token in the cache
    When the system refreshes and receives fewer scopes than before
    Then the system accepts the new token
    And the command may fail at the consumer's authorization layer

  # --- Cache Integrity ---

  Scenario: Cache file is corrupted
    Given a cache file with invalid content
    When any authenticated command is executed
    Then the system rejects the cache
    And the user is prompted to run login again

  Scenario: Cache file has wrong permissions in strict mode
    Given strict permission mode is enabled
    And a cache file with permissions other than 0600
    When any authenticated command is executed
    Then the system rejects the cache
    And the user is prompted to run login again

  Scenario: Cache file has wrong permissions in lenient mode
    Given lenient permission mode is enabled
    And a cache file with permissions other than 0600
    When any authenticated command is executed
    Then the system logs a warning about file permissions
    And attempts to fix permissions to 0600
    And proceeds with the cached token

  # --- Multi-Server Cache ---

  Scenario: First server login becomes default
    Given no previous logins exist
    When the user logs in to server-a.example.com
    Then server-a.example.com is set as the default server
    And subsequent commands target server-a.example.com without --server flag

  Scenario: Additional server login requires explicit targeting
    Given a default server is already configured
    When the user logs in to server-b.example.com
    Then server-b.example.com is stored in the cache
    And the default server remains unchanged
    And commands must use --server server-b.example.com to target it

  Scenario: Changing the default server
    Given logins exist for server-a and server-b
    When the user runs config set-default server-b.example.com
    Then server-b.example.com becomes the default
    And subsequent commands without --server target server-b.example.com

  Scenario: Command with explicit server uses correct token
    Given logins exist for server-a and server-b
    When the user runs a command with --server server-b.example.com
    Then the system uses the cached token for server-b.example.com
    And does not use the default server's token
