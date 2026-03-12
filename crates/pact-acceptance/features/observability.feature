Feature: Observability
  pact uses three observability channels: journal metrics to Prometheus,
  config events to Loki, and agent health via lattice-node-agent eBPF.
  No per-agent Prometheus scraping (ADR-005).

  Background:
    Given a journal with default state

  # --- Prometheus metrics (journal server only) ---

  Scenario: Journal exposes Raft health metrics
    When the metrics endpoint is queried
    Then the response should include "pact_raft_leader" gauge
    And the response should include "pact_raft_term" gauge
    And the response should include "pact_raft_log_entries" gauge
    And the response should include "pact_raft_replication_lag" gauge

  Scenario: Journal exposes config metrics
    When the metrics endpoint is queried
    Then the response should include "pact_journal_entries_total" counter
    And the response should include "pact_journal_boot_streams_active" gauge
    And the response should include "pact_journal_boot_stream_duration_seconds" histogram

  Scenario: Journal exposes overlay build metrics
    When the metrics endpoint is queried
    Then the response should include "pact_journal_overlay_builds_total" counter

  Scenario: Metrics served on port 9091 (not 9090)
    When the journal starts with default config
    Then the metrics endpoint should be available on port 9091

  # --- Health endpoint ---

  Scenario: Health endpoint returns 200 when healthy
    Given a healthy journal node
    When GET /health is requested
    Then the response status should be 200
    And the response body should include the Raft role

  Scenario: Health endpoint indicates leader status
    Given a journal node that is the Raft leader
    When GET /health is requested
    Then the response should indicate role "leader"

  Scenario: Health endpoint indicates follower status
    Given a journal node that is a Raft follower
    When GET /health is requested
    Then the response should indicate role "follower"

  # --- Loki event streaming ---

  Scenario: Config commit events forwarded to Loki
    Given Loki forwarding is enabled
    When a config commit is recorded
    Then a structured JSON event should be sent to Loki
    And the event should have label component "journal"
    And the event should include entry_type, scope, author, and sequence

  Scenario: Admin operations forwarded to Loki
    Given Loki forwarding is enabled
    When an exec operation is recorded
    Then a structured JSON event should be sent to Loki

  Scenario: Emergency events forwarded to Loki
    Given Loki forwarding is enabled
    When emergency mode is entered
    Then a structured JSON event should be sent to Loki
    And the event should include the emergency reason

  Scenario: Loki forwarding is optional
    Given Loki forwarding is disabled
    When a config commit is recorded
    Then no event should be sent to Loki
    And the commit should still be recorded in the journal

  # --- No agent-level Prometheus (ADR-005) ---

  Scenario: Agent does not expose Prometheus metrics endpoint
    Given a running pact-agent
    Then the agent should not expose a /metrics endpoint
    And agent health should be monitored via lattice-node-agent eBPF

  # --- Grafana dashboard data ---

  Scenario: Fleet configuration health data available
    Given nodes in various config states
    When fleet health is queried
    Then the data should support a drift heatmap
    And the data should show commit activity over time

  Scenario: Admin operations data available
    Given exec and shell operations in the audit log
    When admin operations are queried
    Then the data should show operation frequency
    And the data should show whitelist violations

  Scenario: Emergency session data available
    Given active and completed emergency sessions
    When emergency data is queried
    Then the data should show active emergency count
    And the data should show session durations
