# Drift Detection Architecture

## Overview

Drift detection is pact's core mechanism for tracking configuration state divergence.
It follows a blacklist-first approach (ADR-002): observe everything, exclude known noise.

## Drift Vector

Seven dimensions tracked independently:

| Dimension | Source | Weight | Example |
|-----------|--------|--------|---------|
| kernel | sysctl changes | 2.0 | vm.swappiness modified |
| mounts | mount/unmount events | 1.0 | NFS share mounted |
| files | file create/modify/delete | 1.0 | /etc/ntp.conf changed |
| network | interface changes | 1.0 | eth0 link state change |
| services | process start/stop | 1.0 | nginx started |
| packages | package install/remove | 1.0 | CUDA toolkit updated |
| gpu | GPU state changes | 2.0 | GPU health degraded |

**Magnitude**: Weighted L2 norm of the drift vector.
Kernel and GPU have 2x weight (higher impact on node behavior).

## Observer Pipeline

```
Observer → ObserverEvent → DriftEvaluator → CommitWindowManager
   │                            │                    │
   ├─ InotifyObserver (files)   ├─ blacklist filter   ├─ window = base / (1 + mag * sens)
   ├─ NetlinkObserver (network) ├─ category mapping   ├─ Idle → Open → Expired
   └─ EbpfObserver (kernel)     └─ magnitude calc     └─ emergency extends window
```

## Blacklist Patterns

Default patterns (noise suppression):
```
/tmp/**
/var/log/**
/proc/**
/sys/**
/dev/**
/run/user/**
```

Pattern matching:
- `**` = recursive match (any depth)
- `/*` = single path segment
- Exact paths = literal match

Blacklist is dynamically updateable via config subscription from journal.

## Commit Window

Formula: `window_seconds = base_window / (1 + drift_magnitude * sensitivity)`

| Drift | Sensitivity=2.0 | Base=900s | Window |
|-------|-----------------|-----------|--------|
| 0.0 | - | 900s | Idle (no window) |
| 0.5 | 2.0 | 900s | 450s |
| 1.0 | 2.0 | 900s | 300s |
| 5.0 | 2.0 | 900s | 82s |

Minimum window: 60 seconds (clamped).
Emergency mode: window extended to `emergency_window_seconds` (default 4 hours).

## Conflict Resolution (CR1-CR3)

On partition reconnect:
1. Agent compares local state against journal entries
2. Conflicting keys are registered in ConflictManager
3. Grace period: admin resolves per-key (AcceptLocal | AcceptJournal)
4. Auto-resolve: journal-wins after grace period expires
5. All resolutions logged for audit

## Homogeneity Check (ND3)

vCluster nodes should have identical config. Per-node deltas (node-scoped entries)
indicate heterogeneity. `check_homogeneity()` reports nodes with per-node deltas
that diverge from the vCluster overlay.
