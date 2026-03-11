# ADR-002: Blacklist-First Drift Detection with Observe-Only Bootstrap

## Status: Accepted

## Decision

Monitor all system state changes by default. Blacklist known-safe operational changes.
Initial deployment in observe-only mode: detect and log everything, enforce nothing.
Build empirical blacklist from real traffic before enabling enforcement.

Default blacklist: /tmp/**, /var/log/**, /proc/**, /sys/**, /dev/**, /run/user/**

Transition to enforcement per-vCluster:
```toml
enforcement_mode = "observe"  # then "enforce"
```
