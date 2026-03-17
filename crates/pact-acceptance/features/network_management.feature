Feature: Network Management
  pact-agent configures network interfaces via netlink when running as init
  (PactSupervisor mode). In systemd mode, network configuration is delegated
  to the existing network manager. (Invariants: NM1-NM2)

  # --- Netlink configuration ---

  Scenario: Configure network interface via netlink
    Given a supervisor with backend "pact"
    And the overlay declares interface "eth0" with:
      | address    | 10.0.1.42/24   |
      | gateway    | 10.0.1.1       |
      | mtu        | 9000           |
    When the ConfigureNetwork boot phase executes
    Then interface "eth0" should have address "10.0.1.42/24"
    And interface "eth0" should have MTU 9000
    And a default route via "10.0.1.1" should exist
    And interface "eth0" should be in state "Up"

  Scenario: Multiple interfaces configured
    Given a supervisor with backend "pact"
    And the overlay declares interfaces "eth0" and "eth1"
    When the ConfigureNetwork boot phase executes
    Then both interfaces should be configured and up

  Scenario: Network configuration failure blocks boot
    Given a supervisor with backend "pact"
    And interface "eth0" configuration will fail (driver error)
    When the ConfigureNetwork boot phase executes
    Then the boot phase should fail
    And no subsequent boot phases should start
    And an AuditEvent should be emitted with detail "network configuration failed"

  # --- Systemd mode delegation ---

  Scenario: Systemd mode does not use netlink
    Given a supervisor with backend "systemd"
    When pact-agent starts
    Then pact-agent should not configure any network interfaces
    And network configuration should be handled by the existing network manager

  # --- Network before services ---

  Scenario: Network-dependent services wait for network
    Given a service "lattice-node-agent" that requires network
    And the network is not yet configured
    When the StartServices boot phase begins
    Then "lattice-node-agent" should not start until network is up

  # --- Runtime network changes ---

  Scenario: Overlay update reconfigures network interface
    Given a supervisor with backend "pact"
    And interface "eth0" is configured with MTU 1500
    When a new overlay changes eth0 MTU to 9000
    Then interface "eth0" MTU should be updated to 9000 via netlink
    And an AuditEvent should be emitted for the network change

  Scenario: Link loss detected
    Given interface "eth0" is in state "Up"
    When the physical link is lost
    Then interface "eth0" should transition to state "Down"
    And a drift event should be recorded for the network dimension
