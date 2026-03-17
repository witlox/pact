Feature: Process Supervisor
  pact-agent includes a built-in process supervisor (PactSupervisor) that
  manages services on compute nodes. It replaces systemd for diskless HPC
  nodes that run 5-9 services. systemd is available as a fallback. (ADR-006)
  (Invariants: PS1-PS3, A6-A7)

  Background:
    Given a supervisor with backend "pact"

  # --- Service lifecycle ---

  Scenario: Start a service
    Given a service declaration for "chronyd" with binary "/usr/sbin/chronyd"
    When the service "chronyd" is started
    Then the service "chronyd" should be in state "Running"

  Scenario: Stop a running service
    Given a running service "chronyd"
    When the service "chronyd" is stopped
    Then the service "chronyd" should be in state "Stopped"

  Scenario: Restart a running service
    Given a running service "chronyd"
    When the service "chronyd" is restarted
    Then the service "chronyd" should be in state "Running"
    And the service should have been stopped and started

  # --- Health checks ---

  Scenario: Process health check passes for running service
    Given a running service "chronyd" with health check type "Process"
    When a health check is performed for "chronyd"
    Then the health check should pass

  Scenario: HTTP health check for service with endpoint
    Given a running service "lattice-node-agent" with health check type "Http" at "http://localhost:9100/health"
    When a health check is performed for "lattice-node-agent"
    Then the health check should be evaluated against the HTTP endpoint

  Scenario: TCP health check for service with port
    Given a running service "metrics-exporter" with health check type "Tcp" on port 9090
    When a health check is performed for "metrics-exporter"
    Then the health check should be evaluated against the TCP port

  # --- Restart policies ---

  Scenario: Service with restart policy "Always" restarts on failure
    Given a service "chronyd" with restart policy "Always" and delay 5 seconds
    When the service "chronyd" fails
    Then the service "chronyd" should be restarted after 5 seconds

  Scenario: Service with restart policy "OnFailure" restarts on crash
    Given a service "worker" with restart policy "OnFailure" and delay 2 seconds
    When the service "worker" exits with non-zero code
    Then the service "worker" should be restarted after 2 seconds

  Scenario: Service with restart policy "OnFailure" stays stopped on clean exit
    Given a service "oneshot" with restart policy "OnFailure" and delay 2 seconds
    When the service "oneshot" exits with code 0
    Then the service "oneshot" should remain in state "Stopped"

  Scenario: Service with restart policy "Never" stays stopped
    Given a service "batch" with restart policy "Never" and delay 0 seconds
    When the service "batch" fails
    Then the service "batch" should remain in state "Failed"

  # --- Dependency ordering ---

  Scenario: Services start in dependency order
    Given a service "chronyd" with order 1 and no dependencies
    And a service "nvidia-persistenced" with order 2 and no dependencies
    And a service "lattice-node-agent" with order 10 and depends on "chronyd"
    When all services are started
    Then "chronyd" should start before "lattice-node-agent"
    And "nvidia-persistenced" should start before "lattice-node-agent"

  Scenario: Services shut down in reverse dependency order
    Given a running service "chronyd" with order 1
    And a running service "lattice-node-agent" with order 10 and depends on "chronyd"
    When all services are stopped
    Then "lattice-node-agent" should stop before "chronyd"

  # --- Supervisor status ---

  Scenario: Supervisor reports service counts
    Given 3 declared services with 2 running and 1 failed
    When the supervisor status is queried
    Then the status should report backend "Pact"
    And the status should report 3 declared, 2 running, 1 failed

  # --- Backend selection ---

  Scenario: Supervisor backend is configurable per vCluster
    Given supervisor config with backend "pact"
    Then the supervisor should use the "Pact" backend

  Scenario: Systemd backend is available as fallback
    Given supervisor config with backend "systemd"
    Then the supervisor should use the "Systemd" backend

  # --- Supervision loop ---

  Scenario: Supervision loop detects crashed service and restarts
    Given a running service "nvidia-persistenced" with restart policy "Always"
    When "nvidia-persistenced" crashes with exit code 1
    Then the supervision loop should detect the crash within the poll interval
    And "nvidia-persistenced" should be restarted after the configured delay
    And the restart count should be incremented
    And an AuditEvent should be emitted for the crash and restart

  Scenario: Supervision loop respects OnFailure policy for clean exit
    Given a running service "oneshot" with restart policy "OnFailure"
    When "oneshot" exits with code 0
    Then the supervision loop should detect the exit
    And "oneshot" should not be restarted
    And the service should be in state "Stopped"

  Scenario: Supervision loop does not run in systemd mode
    Given a supervisor with backend "systemd"
    When a service crashes
    Then pact should not attempt to restart the service
    And systemd should handle the restart via native Restart= directive

  # --- Real compute node service sets ---

  Scenario: ML training vCluster starts 7 services
    Given a vCluster "ml-training" with service declarations:
      | name                  | order | restart_policy | cgroup_slice          |
      | chronyd               | 1     | Always         | pact.slice/infra      |
      | dbus-daemon           | 2     | Always         | pact.slice/infra      |
      | cxi_rh-0              | 3     | Always         | pact.slice/network    |
      | cxi_rh-1              | 3     | Always         | pact.slice/network    |
      | cxi_rh-2              | 3     | Always         | pact.slice/network    |
      | cxi_rh-3              | 3     | Always         | pact.slice/network    |
      | nvidia-persistenced   | 4     | Always         | pact.slice/gpu        |
      | nv-hostengine         | 5     | Always         | pact.slice/gpu        |
      | rasdaemon             | 6     | OnFailure      | pact.slice/infra      |
      | lattice-node-agent    | 10    | Always         | workload              |
    When all services are started
    Then all 10 service instances should be in state "Running"
    And services should have started in order 1, 2, 3, 4, 5, 6, 10

  Scenario: Regulated vCluster adds audit services
    Given a vCluster "regulated" extending "ml-training" with:
      | name                | order | restart_policy | cgroup_slice       |
      | auditd              | 7     | Always         | pact.slice/audit   |
      | audit-forwarder     | 8     | Always         | pact.slice/audit   |
    When all services are started
    Then 12 service instances should be running
    And auditd should start after rasdaemon and before lattice-node-agent

  Scenario: Dev sandbox vCluster starts minimal services
    Given a vCluster "dev-sandbox" with service declarations:
      | name                | order | restart_policy | cgroup_slice       |
      | chronyd             | 1     | Always         | pact.slice/infra   |
      | dbus-daemon         | 2     | Always         | pact.slice/infra   |
      | cxi_rh-0            | 3     | Always         | pact.slice/network |
      | rasdaemon           | 4     | OnFailure      | pact.slice/infra   |
      | lattice-node-agent  | 10    | Always         | workload           |
    When all services are started
    Then 5 service instances should be running

  # --- Service lifecycle journal entries ---

  Scenario: Service start is recorded in journal
    When the service "chronyd" is started by "admin@example.com"
    Then a ServiceLifecycle entry should be recorded
    And the entry should record action "ServiceStart" for service "chronyd"

  Scenario: Service restart is recorded in journal
    Given a running service "chronyd"
    When the service "chronyd" is restarted by "admin@example.com"
    Then a ServiceLifecycle entry should be recorded
    And the entry should record action "ServiceRestart" for service "chronyd"
