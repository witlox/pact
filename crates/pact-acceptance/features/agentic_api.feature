Feature: Agentic API (MCP)
  pact exposes an MCP server for AI agent tool-use. It wraps pact gRPC
  APIs with 10 tools. Authenticated as pact-service-ai with limited writes.

  Background:
    Given a journal with default state
    And an MCP server with pact-service-ai identity

  # --- Read operations ---

  Scenario: AI agent queries fleet status
    Given nodes in various config states
    When the AI agent calls pact_status
    Then the response should include node states and vCluster info

  Scenario: AI agent queries drift details
    Given node "node-001" has active drift
    When the AI agent calls pact_diff for node "node-001"
    Then the response should include the drift vector

  Scenario: AI agent queries config history
    Given 10 config entries in the journal
    When the AI agent calls pact_log with limit 5
    Then the response should include 5 entries

  Scenario: AI agent queries fleet-wide health
    Given a fleet of 100 nodes
    When the AI agent calls pact_query_fleet for degraded GPUs
    Then the response should include nodes with degraded GPU health

  Scenario: AI agent queries service status
    When the AI agent calls pact_service_status for "chronyd"
    Then the response should include the service state across nodes

  # --- Write operations (require authorization) ---

  Scenario: AI agent commits change when policy allows
    Given the policy allows AI-initiated commits for vCluster "ml-training"
    When the AI agent calls pact_commit for vCluster "ml-training"
    Then the commit should succeed
    And the author should be recorded as "service/ai-agent"

  Scenario: AI agent applies config when policy allows
    Given the policy allows AI-initiated applies for vCluster "dev-sandbox"
    When the AI agent calls pact_apply with a config spec
    Then the apply should succeed

  Scenario: AI agent rollback when policy allows
    Given the policy allows AI-initiated rollbacks for vCluster "dev-sandbox"
    When the AI agent calls pact_rollback to sequence 5
    Then the rollback should succeed

  # --- Restricted operations ---

  Scenario: AI agent cannot enter emergency mode
    When the AI agent calls pact_emergency
    Then the request should be denied
    And the denial reason should be "emergency mode restricted to human admins"

  Scenario: AI agent exec requires explicit policy authorization
    Given the policy does not authorize AI exec on vCluster "ml-training"
    When the AI agent calls pact_exec on node "node-001"
    Then the request should be denied

  Scenario: AI agent exec succeeds when policy authorizes
    Given the policy authorizes AI exec for diagnostics on vCluster "dev-sandbox"
    When the AI agent calls pact_exec with command "nvidia-smi" on node "node-001"
    Then the command should execute
    And the author should be "service/ai-agent"

  # --- Audit trail ---

  Scenario: All AI agent operations are logged
    When the AI agent calls pact_status
    And the AI agent calls pact_diff for node "node-001"
    Then both operations should be recorded in the audit log
    And the actor should have principal type "Service"
