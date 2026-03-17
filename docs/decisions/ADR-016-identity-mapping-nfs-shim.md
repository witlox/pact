# ADR-016: Identity Mapping — OIDC-to-POSIX UID/GID Shim for NFS

## Status: Accepted

## Context

pact uses OIDC for all authentication (ADR-008, hpc-auth). OIDC works natively with S3
storage. However, NFS uses POSIX UID/GID for file ownership and access control. On compute
nodes where pact is init (no SSSD), a mapping layer is needed to translate OIDC subjects
to POSIX identities for NFS compatibility.

This is explicitly a **bypass shim**, not a core identity system. It exists solely because
NFS cannot authenticate via OIDC. When storage migrates to pure S3 or NFSv4 with string
identifiers, this subsystem becomes unnecessary.

## Decision

### UidMap in pact-journal

The journal stores a `UidMap` — a table of OIDC subject → POSIX UID/GID mappings. Each
entry is Raft-committed and immutable within a federation membership.

Two assignment models (configurable per vCluster):
- **On-demand** (default): unknown OIDC subject authenticates → pact-policy checks IdP →
  assigns UID from org's precursor range → Raft-commits → propagates to agents.
- **Pre-provisioned** (regulated): admin pre-provisions all users. Unknown subjects
  rejected. Required for sensitive vClusters.

### Federation deconfliction via computed precursor ranges

Each Sovra-federated org gets:
- An `org_index` (sequential, Raft-committed on federation join: 0=local, 1, 2, ...)
- A computed UID precursor: `base_uid + org_index * stride` (default: base_uid=10000, stride=10000)
- A computed GID precursor: `base_gid + org_index * stride` (same formula, same stride)
- UID assignment is sequential within the precursor range (precursor to precursor + stride - 1)

Collision is impossible by construction (sequential org_index, non-overlapping ranges).
Stride is a site-wide configurable default (adjustable before assignments start).

On federation departure: all UidEntries for that org are GC'd from the journal, org_index
becomes reclaimable. NFS files owned by departed org's UIDs become orphaned.

### pact-nss: NSS module via libnss crate

A separate crate (`pact-nss`, cdylib) using the `libnss` 0.9.0 Rust crate provides a
Linux NSS module (`libnss_pact.so.2`) that resolves UID/GID lookups from local files:

- pact-agent writes `/run/pact/passwd.db` and `/run/pact/group.db` to tmpfs at boot
  and on journal subscription updates
- NSS module reads from these files via mmap — zero network calls, ~1μs per lookup
- `/etc/nsswitch.conf`: `passwd: files pact` / `group: files pact`
- Full supplementary group resolution (getgrouplist)

The NSS module is read-only. It never writes, never makes network calls, never blocks.

### Activation conditions

Identity mapping is only active when ALL of:
- SupervisorBackend = Pact (not systemd — SSSD handles it in systemd mode)
- NFS storage is in use on the node

When inactive, no .db files are written, no NSS module is loaded.

## Rationale

**Why not OIDC claims with POSIX attributes?**
- IdP may not be under pact's control (federated environments)
- UidMap must be populated before any user authenticates (NFS files exist at boot)
- OIDC tokens only arrive on authentication, but UIDs are needed always

**Why journal as authority, not IdP sync?**
- The mapping must be consistent across all nodes (same UID everywhere)
- Raft-committed entries are immutable and auditable
- Journal subscription pushes updates to agents in sub-second
- IdP sync is an optional optimization, not the primary mechanism

**Why computed precursor ranges, not configured ranges?**
- Computed = deterministic from org_index, no admin configuration per range
- Sequential org_index = no overlap by construction
- Reclaimable on federation departure (GC org's entries, index reusable)
- Simpler than hash-based mapping (no collision risk in small UID space)

**Why a separate pact-nss crate?**
- `libnss` 0.9.0 is LGPL-3.0. Dynamic linking (cdylib) satisfies LGPL.
- NSS module must be a shared library loaded by glibc — cannot be part of pact-agent binary.
- Minimal dependencies (libc, lazy_static, paste).

## Trade-offs

- (+) OIDC-native design — no SSSD, no LDAP on compute nodes
- (+) Consistent UIDs across all nodes (Raft-committed)
- (+) Federation deconfliction by construction (no collisions possible)
- (+) NSS module is pure read-only mmap — no performance impact
- (+) Explicitly a shim — removable when NFS is replaced by S3
- (+) Reclaimable ranges on federation departure
- (-) Additional crate (pact-nss) and shared library to deploy
- (-) UidMap adds entries to journal Raft state
- (-) Stride change after assignments requires UID remapping (operational pain)
- (-) NFS files from departed orgs become orphaned (admin responsibility)
- (-) Sub-second propagation lag for new UIDs (A-Id3, F32)

## Consequences

- pact-journal Raft state gains `UidMap` and `OrgIndex` entries
- pact-agent gains `identity/` submodule for UidMap management
- New crate: `pact-nss` (cdylib, LGPL-3.0 compatible)
- SquashFS images must include `libnss_pact.so.2` and nsswitch.conf entry
- IdP sync (SCIM/LDAP) is an optional journal-side optimization, not required
- Boot Phase 3 (LoadIdentity) loads UidMap before Phase 5 (StartServices)
- Services with non-root users wait for UidMap resolution (IM7)

## References

- specs/domain-model.md §2c (Identity Mapping context)
- specs/invariants.md IM1-IM7
- specs/assumptions.md A-Id1 through A-Id6
- specs/features/identity_mapping.feature (17 scenarios)
- specs/failure-modes.md F24 (range exhaustion), F25 (NSS .db corruption), F32 (propagation lag)
- libnss crate: https://lib.rs/crates/libnss (0.9.0, Feb 2025)
