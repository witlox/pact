# Observability

## Design: No agent-level Prometheus scraping

Three channels:
1. Journal server metrics → Prometheus → Grafana (3-5 scrape targets)
2. Config + admin events → Journal → Loki → Grafana (event stream)
3. Agent process health → lattice-node-agent eBPF → existing Prometheus

## Grafana Dashboards

- Fleet Configuration Health: drift heatmap, commit activity, boot performance
- Admin Operations: exec/shell session frequency, command whitelist violations
- Emergency Sessions: active, duration, stale alerts
- Journal Health: Raft quorum, log growth, replication lag

## Alerting

Critical: quorum loss, stale emergency
Warning: high drift rate, slow boot config, policy auth failures, GPU degradation
