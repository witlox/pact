# ADR-005: No Agent-Level Prometheus Metrics

## Status: Accepted

## Decision

No per-agent Prometheus scraping (would be 10k targets). Three channels instead:

1. Journal server metrics → Prometheus → Grafana (3-5 targets)
2. Config events → Journal → Loki → Grafana (event stream)
3. Agent process health → lattice-node-agent eBPF → existing Prometheus
