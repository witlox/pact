#!/usr/bin/env bash
# install-monitoring.sh — Install Prometheus + Grafana + Loki on admin node.
#
# Reusable on-prem. No GCP-specific logic.
#
# Usage: ./install-monitoring.sh <journal-hosts>
#   journal-hosts: Comma-separated journal hostnames for scrape targets

set -euo pipefail

JOURNAL_HOSTS="${1:?Usage: install-monitoring.sh <journal-hosts>}"

echo "=== Installing monitoring stack ==="

# Install Docker (needed for Loki and Dex)
if ! command -v docker &>/dev/null; then
    echo "Installing Docker..."
    apt-get update -qq
    apt-get install -y -qq ca-certificates curl gnupg
    install -m 0755 -d /etc/apt/keyrings
    curl -fsSL https://download.docker.com/linux/debian/gpg -o /etc/apt/keyrings/docker.asc
    chmod a+r /etc/apt/keyrings/docker.asc
    echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] \
        https://download.docker.com/linux/debian $(. /etc/os-release && echo "$VERSION_CODENAME") stable" \
        > /etc/apt/sources.list.d/docker.list
    apt-get update -qq
    apt-get install -y -qq docker-ce docker-ce-cli containerd.io
    systemctl enable docker
    systemctl start docker
    echo "Docker installed"
fi

# Install Prometheus
if ! command -v prometheus &>/dev/null; then
    echo "Installing Prometheus..."
    apt-get update -qq && apt-get install -y -qq prometheus
fi

# Generate scrape config for journal nodes
IFS=',' read -ra HOSTS <<< "$JOURNAL_HOSTS"
TARGETS=""
for h in "${HOSTS[@]}"; do
    TARGETS="${TARGETS}      - '${h}:9091'\n"
done

cat > /etc/prometheus/prometheus.yml <<EOF
global:
  scrape_interval: 15s
  evaluation_interval: 15s

rule_files:
  - /etc/prometheus/rules/*.yml

scrape_configs:
  - job_name: 'pact-journal'
    static_configs:
      - targets:
$(printf "$TARGETS")

  - job_name: 'prometheus'
    static_configs:
      - targets: ['localhost:9090']
EOF

# Copy alert rules if available
if [ -d /opt/pact/alerting ]; then
    mkdir -p /etc/prometheus/rules
    cp /opt/pact/alerting/*.yml /etc/prometheus/rules/ 2>/dev/null || true
fi

systemctl restart prometheus
echo "Prometheus configured and started"

# Install Grafana
if ! command -v grafana-server &>/dev/null; then
    echo "Installing Grafana..."
    apt-get install -y -qq apt-transport-https software-properties-common
    wget -q -O /usr/share/keyrings/grafana.key https://apt.grafana.com/gpg.key
    echo "deb [signed-by=/usr/share/keyrings/grafana.key] https://apt.grafana.com stable main" \
        > /etc/apt/sources.list.d/grafana.list
    apt-get update -qq && apt-get install -y -qq grafana
fi

# Provision dashboards
if [ -d /opt/pact/grafana ]; then
    mkdir -p /etc/grafana/provisioning/dashboards /etc/grafana/provisioning/datasources
    cp /opt/pact/grafana/provisioning/*.yml /etc/grafana/provisioning/dashboards/ 2>/dev/null || true
    cp /opt/pact/grafana/provisioning/datasources.yml /etc/grafana/provisioning/datasources/ 2>/dev/null || true
    mkdir -p /var/lib/grafana/dashboards
    cp /opt/pact/grafana/dashboards/*.json /var/lib/grafana/dashboards/ 2>/dev/null || true
fi

systemctl enable grafana-server
systemctl start grafana-server
echo "Grafana started on :3000"

# Install Loki (via Docker or binary)
if command -v docker &>/dev/null; then
    echo "Starting Loki via Docker..."
    docker run -d --name loki --restart unless-stopped \
        -p 3100:3100 \
        grafana/loki:3.4.2 \
        -config.file=/etc/loki/local-config.yaml
    echo "Loki started on :3100"
else
    echo "Docker not available — skipping Loki (install manually)"
fi

# Install Dex OIDC IdP (for pact CLI auth)
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
if [ -f "$SCRIPT_DIR/install-dex.sh" ]; then
    "$SCRIPT_DIR/install-dex.sh"
elif [ -f /opt/pact/deploy/install-dex.sh ]; then
    /opt/pact/deploy/install-dex.sh
else
    echo "WARNING: install-dex.sh not found — skipping Dex OIDC setup"
fi

echo "=== Monitoring stack ready ==="
echo "  Prometheus: http://localhost:9090"
echo "  Grafana:    http://localhost:3000 (admin/admin)"
echo "  Loki:       http://localhost:3100"
echo "  Dex OIDC:   http://$(hostname -I | awk '{print $1}'):5556/dex"
