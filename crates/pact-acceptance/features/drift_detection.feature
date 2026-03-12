Feature: Drift Detection
  pact uses a blacklist-first approach: monitor all system state changes,
  exclude known-safe operational patterns, and report everything else as drift.
  Drift is tracked across 7 dimensions with weighted magnitude.

  Background:
    Given a journal with default state
    And default drift weights

  # --- Blacklist filtering (ADR-002) ---

  Scenario: Changes in blacklisted paths are ignored
    When a file change is detected at "/tmp/scratch/data.bin"
    Then the change should be filtered by the blacklist
    And no drift event should be emitted

  Scenario: Changes in /var/log are ignored
    When a file change is detected at "/var/log/syslog"
    Then the change should be filtered by the blacklist

  Scenario: Changes in /proc are ignored
    When a file change is detected at "/proc/sys/vm/swappiness"
    Then the change should be filtered by the blacklist

  Scenario: Changes outside blacklist trigger drift
    When a file change is detected at "/etc/pact/agent.toml"
    Then a drift event should be emitted
    And the drift should be in the "files" dimension

  Scenario: Custom blacklist patterns are respected
    Given a custom blacklist pattern "/opt/cache/**"
    When a file change is detected at "/opt/cache/model.bin"
    Then the change should be filtered by the blacklist

  # --- 7-dimension drift vector ---

  Scenario: Mount change produces drift in mounts dimension
    When a mount change is detected for "/scratch"
    Then the drift vector should have non-zero "mounts" magnitude
    And other dimensions should be zero

  Scenario: Kernel parameter change produces drift in kernel dimension
    When a kernel parameter change is detected for "vm.swappiness"
    Then the drift vector should have non-zero "kernel" magnitude

  Scenario: Service state change produces drift in services dimension
    When a service state change is detected for "chronyd"
    Then the drift vector should have non-zero "services" magnitude

  Scenario: Network change produces drift in network dimension
    When a network interface change is detected for "eth0"
    Then the drift vector should have non-zero "network" magnitude

  Scenario: GPU state change produces drift in gpu dimension
    When a GPU state change is detected for GPU index 0
    Then the drift vector should have non-zero "gpu" magnitude

  # --- Weighted magnitude ---

  Scenario: Kernel drift is weighted higher than file drift
    Given a drift vector with kernel magnitude 1.0
    And a drift vector with files magnitude 1.0
    Then the kernel drift total magnitude should be greater than the files drift total magnitude

  Scenario: GPU drift is weighted higher than mount drift
    Given a drift vector with gpu magnitude 1.0
    And a drift vector with mounts magnitude 1.0
    Then the gpu drift total magnitude should be greater than the mounts drift total magnitude

  Scenario: Zero drift produces zero magnitude
    Given a drift vector with all dimensions at 0.0
    Then the total drift magnitude should be 0.0

  Scenario: Multi-dimension drift compounds magnitude
    Given a drift vector with kernel magnitude 0.5 and gpu magnitude 0.5
    Then the total drift magnitude should be greater than a single dimension at 0.5

  # --- Observe-only mode (ADR-002 bootstrap) ---

  Scenario: Observe-only mode logs drift without enforcement
    Given enforcement mode is "observe"
    When drift is detected on node "node-001"
    Then the drift should be logged
    And no rollback should be triggered

  Scenario: Enforce mode triggers commit window on drift
    Given enforcement mode is "enforce"
    When drift is detected on node "node-001"
    Then a commit window should be opened
    And the drift should be logged

  # --- Drift from multiple sources ---

  Scenario: eBPF-detected syscall change triggers drift
    When an eBPF probe detects a sethostname syscall
    Then a drift event should be emitted in the "kernel" dimension

  Scenario: inotify file watch triggers drift
    When an inotify event fires for "/etc/resolv.conf"
    Then a drift event should be emitted in the "files" dimension

  Scenario: netlink interface change triggers drift
    When a netlink event reports interface "eth0" going down
    Then a drift event should be emitted in the "network" dimension
