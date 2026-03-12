Feature: Process Supervisor
  pact-agent includes a built-in process supervisor (PactSupervisor) that
  manages services on compute nodes. It replaces systemd for diskless HPC
  nodes that run 4-7 services. systemd is available as a fallback. (ADR-006)

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
