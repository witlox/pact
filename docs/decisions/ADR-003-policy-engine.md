# ADR-003: Policy Engine Choice — OPA/Rego

## Status: Accepted

## Context

pact-policy needs a policy evaluation engine for authorization decisions: who can
commit, exec, shell, start emergency mode, etc. Two candidates evaluated:

- **OPA/Rego**: mature, widely adopted, REST API, used by Sovra for federation
- **Cedar**: newer (AWS), Rust-native, strongly typed, no REST overhead

## Decision

**OPA/Rego as a sidecar process.** pact-policy calls OPA via REST on localhost.

## Rationale

1. **Sovra compatibility**: Sovra uses OPA. Federated policy templates need a shared
   language. Using OPA means pact and Sovra speak the same policy format — Rego
   templates authored once, federated across sites without translation.

2. **Sidecar model**: OPA runs as a separate process alongside pact-journal or
   pact-policy. pact calls `http://localhost:8181/v1/data/pact/<decision>` via
   reqwest. This keeps pact's Rust codebase free of policy language interpreters.

3. **Not on the hot path**: Policy evaluation happens on admin operations (exec,
   commit, shell session start) — not on every boot config read or heartbeat.
   The REST overhead (~1ms localhost) is negligible.

4. **Operational maturity**: OPA has established patterns for testing policies
   (opa test), debugging (opa eval), and distributing bundles (OPA bundles API).

## Deployment

OPA runs on journal/policy nodes alongside pact-journal, **not on compute nodes**.

Policy evaluation flow for admin operations:
```
CLI → pact-agent (ExecRequest/ShellSessionRequest via gRPC)
  → agent calls PolicyService.Evaluate() on pact-policy node
  → pact-policy calls OPA via localhost REST
  → OPA evaluates Rego rules → decision returned to agent
  → agent enforces decision (proceed or deny)
```

The agent is the entry point for exec/shell (it owns the PTY and process
execution), but delegates authorization to the policy service via gRPC.
pact-policy is a library crate linked into the pact-journal binary —
PolicyService runs in-process with the journal, not as a separate deployment.

```
pact-journal node:
  pact-journal (port 9443/9444)
  opa (port 8181, localhost only)
    - loads policy bundles from /etc/pact/policies/ or Sovra sync
    - data: pact pushes current state as OPA data
```

OPA lifecycle on journal nodes depends on the deployment model:
- **systemd deployments**: OPA runs as a systemd service alongside pact-journal
- **pact-managed deployments**: OPA is declared as a supervised service in the
  management node's service declarations, managed by PactSupervisor like any
  other service
- **Container deployments**: OPA runs as a sidecar container

In all cases, OPA is co-located with pact-journal/pact-policy on management
nodes. Compute nodes do not run OPA — they enforce the authorization decisions
received from the policy layer.

## Policy Caching and Partition Resilience

pact-agent receives `VClusterPolicy` as part of the boot config overlay (Phase 1).
This cached policy contains the data needed for local authorization decisions:
- `role_bindings`: OIDC role → allowed actions mapping
- `exec_whitelist` / `shell_whitelist`: allowed commands
- `regulated`, `two_person_approval`: enforcement flags

**Normal operation**: agent calls `PolicyService.Evaluate()` on the policy node
for full OPA/Rego evaluation (complex rules, cross-vCluster checks, approval
workflows).

**Degraded operation** (policy service unreachable): agent falls back to cached
`VClusterPolicy` for basic RBAC decisions:
- Whitelist checks: allowed (command in cached whitelist + role has action)
- Two-person approval: denied (cannot verify without policy service)
- Complex Rego rules: denied (cannot evaluate without OPA)
- Platform admin override: allowed (role cached, but logged as degraded)

This follows the same AP consistency model as config: nodes keep operating with
cached state during partitions. The agent logs all degraded-mode authorization
decisions and replays them to the journal when connectivity is restored.

Policy cache is refreshed on each successful `PolicyService.Evaluate()` call
and on explicit policy update events streamed from the journal.

## Trade-offs

- (+) Sovra federation works natively — same Rego language
- (+) Rich ecosystem (testing, debugging, bundle distribution)
- (+) No Rust bindings to maintain — clean REST boundary
- (-) Extra process to deploy and monitor on management nodes
- (-) REST latency vs in-process Cedar evaluation (acceptable — not hot path)
- (-) Rego learning curve for operators writing custom policies

## Rego Policy Structure

```
pact/
  authz/
    exec.rego          # Who can exec on which vClusters
    commit.rego        # Commit authorization + two-person approval
    shell.rego         # Shell session authorization
    emergency.rego     # Emergency mode restrictions
    service.rego       # Service lifecycle authorization
  data/
    roles.json         # OIDC group → pact role mappings
    vclusters.json     # Per-vCluster policy overrides
```
