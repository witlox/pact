Feature: Authentication Login (hpc-auth crate)

  Shared authentication crate for PACT and Lattice. Provides OAuth2 token
  acquisition, caching, and refresh. Consumed by both CLIs as a library.

  Background:
    Given a configured server URL
    And the server exposes an auth discovery endpoint

  # --- Flow Selection ---

  Scenario: Automatic flow selection with browser available
    Given a browser is available on the client machine
    And the IdP supports Authorization Code with PKCE
    When the user initiates login
    Then the system uses the Authorization Code with PKCE flow

  Scenario: Automatic flow selection without browser
    Given no browser is available on the client machine
    And the IdP supports Device Code grant
    When the user initiates login
    Then the system uses the Device Code flow

  Scenario: Explicit device code override
    Given a browser is available on the client machine
    When the user initiates login with --device-code
    Then the system uses the Device Code flow regardless of browser availability

  # --- Cascading Fallback ---

  Scenario: PKCE unavailable, falls back to confidential client
    Given the IdP does not support public clients
    And the IdP supports confidential clients
    When the user initiates login
    Then the system uses Authorization Code with an embedded client secret

  Scenario: Device Code unavailable, falls back to manual paste
    Given no browser is available on the client machine
    And the IdP does not support Device Code grant
    When the user initiates login
    Then the system prints the authorization URL
    And prompts the user to paste the authorization code
    And exchanges the code for a token

  Scenario: No supported flow available
    Given the IdP discovery document lists no supported grant types
    When the user initiates login
    Then the system reports that no compatible authentication flow is available
    And exits with an error

  # --- Authorization Code + PKCE ---

  Scenario: Successful PKCE login
    Given a configured IdP endpoint
    And a browser is available
    And the IdP supports PKCE
    When the user initiates login
    Then the system opens a browser to the IdP authorization URL with a PKCE challenge
    And starts a localhost listener for the callback
    And upon successful authentication stores the token pair in the cache
    And the cache file has 0600 permissions

  Scenario: Localhost callback timeout
    Given the system is waiting for the IdP callback
    When no callback is received within the timeout period
    Then the system reports a timeout error
    And suggests using --device-code as fallback

  # --- Device Code ---

  Scenario: Successful device code login
    Given a configured IdP endpoint
    And the IdP supports Device Code grant
    When the user initiates login via device code flow
    Then the system requests a device code from the IdP
    And displays the verification URL and user code
    And polls the IdP token endpoint until authorized
    And stores the token pair in the cache

  Scenario: Device code expires before user authorizes
    Given the system is polling for device code authorization
    When the device code expires at the IdP
    Then the system reports the code has expired
    And prompts the user to try again

  # --- Service Account ---

  Scenario: Successful service account login
    Given a configured IdP endpoint
    And client_id and client_secret are provided
    When the user initiates login with --service-account
    Then the system exchanges client credentials for an access token
    And stores the token in the cache

  Scenario: Invalid client credentials
    Given a configured IdP endpoint
    And invalid client_id or client_secret are provided
    When the user initiates login with --service-account
    Then the system reports authentication failed
    And does not modify the cache

  # --- Already Logged In ---

  Scenario: Login when already authenticated
    Given a valid non-expired token exists in the cache
    When the user initiates login
    Then the system informs the user they are already logged in
    And does not initiate a new authentication flow

  Scenario: Login with expired refresh token
    Given an expired refresh token exists in the cache
    When the user initiates login
    Then the system proceeds with a full login flow

  # --- IdP Discovery ---

  Scenario: Server provides IdP configuration
    Given the server is reachable
    And no manual IdP configuration exists
    When the user initiates login
    Then the system fetches IdP config from the server discovery endpoint
    And uses the returned IdP URL and client ID for authentication

  Scenario: Server unreachable with manual config
    Given the server is unreachable
    And manual IdP configuration exists
    When the user initiates login
    Then the system uses the manual IdP configuration

  Scenario: Server unreachable without manual config
    Given the server is unreachable
    And no manual IdP configuration exists
    When the user initiates login
    Then the system reports it cannot determine the IdP endpoint
    And suggests configuring it manually

  Scenario: Manual config overrides server discovery
    Given manual IdP configuration exists with override enabled
    When the user initiates login
    Then the system uses the manual IdP configuration
    And does not contact the server discovery endpoint

  Scenario: Cached discovery document used when IdP unreachable
    Given a cached OIDC discovery document exists
    And the IdP discovery endpoint is unreachable
    When the user initiates login
    Then the system uses the cached discovery document
    And proceeds with authentication

  Scenario: Stale discovery document causes auth failure
    Given a cached OIDC discovery document exists
    And the cached document contains outdated endpoint URLs
    When authentication fails due to stale endpoints
    Then the system clears the cached discovery document
    And reports that the IdP configuration may have changed
    And suggests retrying when the IdP is reachable
