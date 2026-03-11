# ADR-001: Separate Raft Quorum from Lattice Scheduler

## Status: Accepted

## Context

pact-journal needs a consensus mechanism. Lattice already runs a Raft quorum.

## Decision

**Separate Raft quorum.** Boot storm (10k concurrent config streams) must not block
the lattice scheduler. Capability update from pact-agent to scheduler is unidirectional
gRPC (no consensus needed), so inter-quorum communication is lightweight.

## Trade-offs

- 3-5 more quorum nodes to operate
- Two Raft groups to monitor
- Shared OIDC provider for IAM

## Revisit

If load testing shows shared quorum can handle boot storms (e.g., reads bypass
consensus), operational simplicity of single quorum may justify merging.
