Feature: Platform Bootstrap
  Boot sequence orchestration when pact-agent is PID 1 on diskless compute
  nodes. Includes hardware watchdog, SPIRE integration, device coldplug,
  and boot readiness signaling. (Invariants: PB1-PB5, PS2)

  # --- Boot phase ordering ---

  Scenario: Boot phases execute in strict order
    Given a supervisor with backend "pact"
    When pact-agent boots as PID 1
    Then the following phases should execute in order:
      | phase              |
      | InitHardware       |
      | ConfigureNetwork   |
      | LoadIdentity       |
      | PullOverlay        |
      | StartServices      |
      | Ready              |

  Scenario: Phase failure blocks subsequent phases
    Given a supervisor with backend "pact"
    And the ConfigureNetwork phase will fail
    When pact-agent boots as PID 1
    Then InitHardware should complete
    And ConfigureNetwork should fail
    And LoadIdentity should not start
    And the boot should be in state "BootFailed"

  Scenario: Failed boot phase can be retried
    Given a boot stuck in BootFailed at ConfigureNetwork
    When the failure condition is resolved
    Then the ConfigureNetwork phase should be retried
    And subsequent phases should proceed on success

  # --- Hardware watchdog ---

  Scenario: Watchdog opened when running as PID 1
    Given a supervisor with backend "pact"
    And /dev/watchdog is available
    When pact-agent starts as PID 1
    Then a WatchdogHandle should be opened
    And the watchdog should be petted periodically

  Scenario: Watchdog not opened in systemd mode
    Given a supervisor with backend "systemd"
    When pact-agent starts
    Then no WatchdogHandle should be opened

  Scenario: Watchdog pet coupled to supervision loop
    Given pact-agent is running as PID 1 with watchdog
    When the supervision loop ticks
    Then the watchdog should be petted as part of the tick
    And the pet interval should be at most T/2 of the watchdog timeout

  Scenario: Stuck supervision loop triggers reboot
    Given pact-agent is running as PID 1 with watchdog timeout 30 seconds
    When the supervision loop hangs for more than 30 seconds
    Then the watchdog timer expires
    And the BMC triggers a hard reboot

  # --- Adaptive supervision loop ---

  Scenario: Idle node uses faster poll interval
    Given pact-agent is running with no active allocations
    When the supervision loop adapts
    Then the poll interval should decrease (faster polling)
    And deeper inspections should be performed (eBPF signals, extended health checks)
    And CPU usage should remain below 2 percent

  Scenario: Active workload uses slower poll interval
    Given pact-agent is running with active allocations
    When the supervision loop adapts
    Then the poll interval should increase (slower polling)
    And only basic status checks should be performed
    And CPU usage should remain below 0.5 percent

  # --- SPIRE integration ---

  Scenario: Bootstrap identity used for initial journal auth
    Given a supervisor with backend "pact"
    And a bootstrap identity from OpenCHAMI
    And SPIRE agent is not yet reachable
    When pact-agent authenticates to journal
    Then the bootstrap identity should be used
    And authentication should succeed

  Scenario: SPIRE SVID replaces bootstrap identity
    Given pact-agent authenticated with bootstrap identity
    And SPIRE agent becomes reachable
    When pact-agent requests SVID from SPIRE
    Then an SVID should be obtained
    And pact-agent should rotate to SPIRE-managed mTLS
    And the bootstrap identity should be discarded

  Scenario: pact functions without SPIRE
    Given a supervisor with backend "pact"
    And no SPIRE agent is available
    When pact-agent boots and authenticates
    Then the bootstrap identity or journal-signed cert should be used
    And all pact functionality should be available
    And SPIRE SVID acquisition should be retried periodically

  # --- Device coldplug ---

  Scenario: Coldplug sets up device nodes at boot
    Given a supervisor with backend "pact"
    When the InitHardware boot phase executes
    Then device nodes should be set up from sysfs
    And kernel modules should be loaded as needed
    And device permissions should be set correctly
    And no persistent hotplug daemon should run

  # --- Readiness signal ---

  Scenario: Readiness signal emitted after all phases complete
    When pact-agent completes all boot phases
    Then a ReadinessSignal should be emitted
    And the CapabilityReport should be sent to journal
    And the node should be available for workload scheduling

  Scenario: Readiness signal not emitted on boot failure
    Given the PullOverlay phase fails
    When boot does not complete
    Then no ReadinessSignal should be emitted
    And the node should not be schedulable

  # --- ADR-017: Network topology enforcement ---

  Scenario: Journal communication precedes HSN availability (ADR-017)
    Given a supervisor with backend "pact"
    When pact-agent boots as PID 1
    Then PullOverlay should complete before StartServices
    And journal communication uses management network only
    And HSN services start only in the StartServices phase

  # --- Resource budget during boot ---

  Scenario: Boot completes within time target
    Given a warm journal (overlay cached)
    When pact-agent boots as PID 1
    Then the time from agent start to Ready should be less than 2 seconds

  # --- Systemd mode boot ---

  Scenario: Systemd mode skips init-specific phases
    Given a supervisor with backend "systemd"
    When pact-agent starts
    Then InitHardware should be skipped (systemd handles it)
    And ConfigureNetwork should be skipped (network manager handles it)
    And LoadIdentity should be skipped (SSSD handles it)
    And PullOverlay should execute (pact-specific)
    And StartServices should execute (pact-managed services only)
    And Ready should execute
