# Summary

[Introduction](README.md)

---

# User Guide

- [Getting Started](usage/getting-started.md)
- [CLI Reference](usage/cli-reference.md)
- [Admin Operations](usage/admin-operations.md)
- [Deployment](usage/deployment.md)
- [Troubleshooting](usage/troubleshooting.md)

---

# Architecture

- [System Architecture](architecture/system-architecture.md)
- [Agent Design](architecture/agent-design.md)
- [Journal Design](architecture/journal-design.md)
- [CLI Design](architecture/cli-design.md)
- [Drift Detection](architecture/drift-detection.md)
- [Shell Server](architecture/shell-server.md)
- [Emergency Mode](architecture/emergency-mode.md)
- [Policy Engine](architecture/policy-engine.md)
- [Security](architecture/security.md)
- [Failure Modes](architecture/failure-modes.md)
- [Observability](architecture/observability.md)
- [Agentic API (MCP)](architecture/agentic-api.md)
- [Federation](architecture/federation.md)
- [Testing Strategy](architecture/testing-strategy.md)

---

# Decisions

- [ADR-001: Separate Raft Quorum](decisions/ADR-001-separate-raft-quorum.md)
- [ADR-002: Blacklist Drift Detection](decisions/ADR-002-blacklist-drift-detection.md)
- [ADR-003: Policy Engine](decisions/ADR-003-policy-engine.md)
- [ADR-004: Emergency Audit Trail](decisions/ADR-004-emergency-audit-trail.md)
- [ADR-005: No Agent Prometheus](decisions/ADR-005-no-agent-prometheus.md)
- [ADR-006: Pact as Init](decisions/ADR-006-pact-as-init.md)
- [ADR-007: No SSH](decisions/ADR-007-no-ssh.md)
- [ADR-008: Node Enrollment & Certificate Lifecycle](decisions/ADR-008-node-enrollment-certificate-lifecycle.md)
- [ADR-009: Overlay Staleness & On-Demand Rebuild](decisions/ADR-009-overlay-staleness-on-demand-rebuild.md)
- [ADR-010: Node Delta TTL Bounds](decisions/ADR-010-node-delta-ttl-bounds.md)
- [ADR-011: Degraded-Mode Policy Evaluation](decisions/ADR-011-degraded-mode-policy-evaluation.md)
- [ADR-012: Merge Conflict Grace Period](decisions/ADR-012-merge-conflict-grace-period.md)
- [ADR-013: Two-Person Approval State Machine](decisions/ADR-013-two-person-approval-state-machine.md)
- [ADR-014: Optimistic Concurrency & Commit Windows](decisions/ADR-014-optimistic-concurrency-commit-windows.md)
- [ADR-015: hpc-core Shared Contracts](decisions/ADR-015-hpc-core-shared-contracts.md)
- [ADR-016: Identity Mapping (OIDC→POSIX)](decisions/ADR-016-identity-mapping-nfs-shim.md)
- [ADR-017: Network Topology (Mgmt vs HSN)](decisions/ADR-017-network-topology-management-vs-hsn.md)
