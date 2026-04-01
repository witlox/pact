Feature: Node management delegation
  As a pact operator
  I want reboot and reimage commands to work against CSM or OpenCHAMI
  So that pact works with the node management backend actually deployed

  Background:
    Given a running journal quorum
    And an authenticated operator with role "pact-ops-default"

  # --- Backend selection ---

  Rule: Backend is selected per deployment, not per node

    Scenario: CSM backend selected via config
      Given the node management backend is set to "csm"
      And CSM is reachable at the configured base URL
      When the operator runs "pact reboot x1000c0s0b0n0"
      Then the reboot request is sent via CAPMC API
      And an audit entry is recorded before the CAPMC call

    Scenario: OpenCHAMI backend selected via config
      Given the node management backend is set to "ochami"
      And OpenCHAMI SMD is reachable at the configured base URL
      When the operator runs "pact reboot x1000c0s0b0n0"
      Then the reboot request is sent via SMD Redfish API
      And an audit entry is recorded before the Redfish call

    Scenario: Backend not configured
      Given the node management backend is not configured
      When the operator runs "pact reboot x1000c0s0b0n0"
      Then the command fails with "node management backend not configured"

  # --- Reboot ---

  Rule: Reboot triggers a power cycle via the configured backend

    Scenario: Reboot via CSM succeeds
      Given the node management backend is set to "csm"
      When the operator runs "pact reboot x1000c0s0b0n0"
      Then CAPMC receives POST /capmc/capmc/v1/xname_reinit
      And the request body contains xname "x1000c0s0b0n0"
      And the command reports success

    Scenario: Reboot via OpenCHAMI succeeds
      Given the node management backend is set to "ochami"
      When the operator runs "pact reboot x1000c0s0b0n0"
      Then SMD receives POST /hsm/v2/State/Components/x1000c0s0b0n0/Actions/PowerCycle
      And the command reports success

    Scenario: Reboot fails when backend is unreachable
      Given the node management backend is set to "csm"
      And CSM is unreachable
      When the operator runs "pact reboot x1000c0s0b0n0"
      Then the command fails with a connection error
      And the audit entry still exists in the journal

  # --- Reimage ---

  Rule: Reimage triggers a fresh boot from the boot infrastructure

    Scenario: Reimage via CSM creates a BOS reboot session
      Given the node management backend is set to "csm"
      When the operator runs "pact reimage x1000c0s0b0n0"
      Then BOS receives POST /bos/v2/sessions with operation "reboot"
      And the session targets xname "x1000c0s0b0n0"
      And the command reports success

    Scenario: Reimage via OpenCHAMI triggers a power cycle
      Given the node management backend is set to "ochami"
      When the operator runs "pact reimage x1000c0s0b0n0"
      Then SMD receives POST /hsm/v2/State/Components/x1000c0s0b0n0/Actions/PowerCycle
      And the command reports success

    Scenario: Reimage via CSM fails when node has no BOS boot history
      Given the node management backend is set to "csm"
      And the node "x1000c0s0b0n0" has no BOS boot record
      When the operator runs "pact reimage x1000c0s0b0n0"
      Then the command fails with "no boot template found for node"

    Scenario: Reimage fails when backend is unreachable
      Given the node management backend is set to "csm"
      And CSM is unreachable
      When the operator runs "pact reimage x1000c0s0b0n0"
      Then the command fails with a connection error
      And the audit entry still exists in the journal

  # --- Node import ---

  Rule: Node import queries HSM regardless of backend type

    Scenario: Node import from CSM
      Given the node management backend is set to "csm"
      And HSM contains 3 compute nodes
      When the operator runs "pact node import"
      Then 3 nodes are enrolled in the journal
      And HSM was queried at /smd/hsm/v2/State/Components

    Scenario: Node import from OpenCHAMI
      Given the node management backend is set to "ochami"
      And HSM contains 3 compute nodes
      When the operator runs "pact node import"
      Then 3 nodes are enrolled in the journal
      And HSM was queried at /hsm/v2/State/Components

  # --- Auth ---

  Rule: Auth token is passed opaquely to the backend

    Scenario: Token forwarded to CSM
      Given the node management backend is set to "csm"
      And a node management token is configured
      When the operator runs "pact reboot x1000c0s0b0n0"
      Then the CAPMC request includes Authorization header with the token

    Scenario: Token forwarded to OpenCHAMI
      Given the node management backend is set to "ochami"
      And a node management token is configured
      When the operator runs "pact reboot x1000c0s0b0n0"
      Then the Redfish request includes Authorization header with the token

    Scenario: No token configured
      Given the node management backend is set to "csm"
      And no node management token is configured
      When the operator runs "pact reboot x1000c0s0b0n0"
      Then the request is sent without an Authorization header

  # --- Audit invariant ---

  Rule: Audit always precedes delegation

    Scenario: Audit entry recorded even when backend call fails
      Given the node management backend is set to "csm"
      And CSM returns HTTP 500 on the reboot call
      When the operator runs "pact reboot x1000c0s0b0n0"
      Then the command fails
      And the journal contains an audit entry for "reboot x1000c0s0b0n0"
