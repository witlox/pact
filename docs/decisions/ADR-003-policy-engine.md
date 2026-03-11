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

```
pact-journal node:
  pact-journal (port 9443/9444)
  opa (port 8181, localhost only)
    - loads policy bundles from /etc/pact/policies/ or Sovra sync
    - data: pact pushes current state as OPA data
```

OPA lifecycle is not managed by pact itself — it runs as a system service
(or supervised by pact-agent if co-located on a compute node, though this
is uncommon). Deployment docs provide systemd unit and container examples.

## Trade-offs

- (+) Sovra federation works natively — same Rego language
- (+) Rich ecosystem (testing, debugging, bundle distribution)
- (+) No Rust bindings to maintain — clean REST boundary
- (-) Extra process to deploy and monitor
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
