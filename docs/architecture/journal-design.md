# Journal Design

## Overview

pact-journal is the distributed, append-only configuration log. Separate Raft group
from lattice scheduler (ADR-001). Single source of truth for declared state.

## Log Structure

ConfigEntry: sequence, timestamp, entry_type, scope, author (OIDC identity),
parent (chain for state reconstruction), state_delta, policy_ref, ttl, emergency_reason.

Entry types: Commit, Rollback, AutoConverge, DriftDetected, CapabilityChange,
PolicyUpdate, BootConfig, EmergencyStart, EmergencyEnd, ExecLog, ShellSession.

Note: ExecLog and ShellSession are new entry types — every remote command and shell
session is recorded in the same immutable log as configuration changes.

## Streaming Boot Config

Two-phase protocol:
- Phase 1: vCluster base overlay (pre-computed, compressed ~100-200 KB, served from any replica)
- Phase 2: node-specific delta (<1 KB)

Read replicas (non-voting Raft learners) for 100k+ boot storms.

## Telemetry

- Config events → Loki (structured JSON with labels)
- Server metrics → Prometheus (Raft health, stream throughput, event counts)

## Backup

WAL + periodic snapshots + export to object storage (S3/NFS).
Full state reconstruction from any snapshot + subsequent WAL.
