# Observability

## Design: No agent-level Prometheus scraping

Three channels:
1. Journal server metrics → Prometheus → Grafana (3-5 scrape targets)
2. Config + admin events → Journal → Loki → Grafana (event stream)
3. Agent process health → lattice-node-agent eBPF → existing Prometheus

## Journal Metrics Endpoint

Each pact-journal server exposes a Prometheus metrics endpoint via axum
(HTTP, default port 9091 — avoids conflict with Prometheus server default on 9090). Metrics include:

- `pact_raft_leader` (gauge): 1 if this node is the Raft leader
- `pact_raft_term` (gauge): current Raft term
- `pact_raft_log_entries` (gauge): total log entries
- `pact_raft_replication_lag` (gauge): entries behind leader, per follower
- `pact_journal_entries_total` (counter): total config entries appended
- `pact_journal_boot_streams_active` (gauge): concurrent boot config streams
- `pact_journal_boot_stream_duration_seconds` (histogram): boot stream latency
- `pact_journal_overlay_builds_total` (counter): overlay pre-computation events

Health check endpoint: `GET /health` returns 200 if Raft is healthy.

## Grafana Dashboards

- Fleet Configuration Health: drift heatmap, commit activity, boot performance
- Admin Operations: exec/shell session frequency, command whitelist violations
- Emergency Sessions: active, duration, stale alerts
- Journal Health: Raft quorum, log growth, replication lag

## Alerting

Critical: quorum loss, stale emergency
Warning: high drift rate, slow boot config, policy auth failures, GPU degradation
