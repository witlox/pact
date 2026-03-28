# Deployment Guide

This guide covers deploying pact in production. pact consists of three components
that need to be deployed:

1. **pact-journal** -- 3 or 5 node Raft quorum (management nodes)
2. **pact-agent** -- every compute node
3. **pact CLI** -- admin workstations

## Automated Deployment (recommended)

Deploy scripts in `scripts/deploy/` automate the full deployment. They are
cloud-agnostic and reusable on bare metal. For GCP-specific infrastructure
(VMs, networking), see `infra/gcp/`.

### OS Requirements

Release binaries are built on Rocky Linux 9 (glibc 2.34). Compatible distributions:
- RHEL 9+ / Rocky 9+ / Alma 9+
- Ubuntu 22.04+
- Debian 12+ (bookworm)
- SLES 15 SP4+

Debian 11 and Ubuntu 20.04 are **not supported** (glibc too old).

Release artifacts per architecture (x86_64 and aarch64):
- `pact-platform-{arch}.tar.gz` — pact (CLI), pact-journal, pact-mcp
- `pact-agent-{arch}-pact.tar.gz` — agent with PactSupervisor (PID 1 mode)
- `pact-agent-{arch}-systemd.tar.gz` — agent with systemd backend

All agent variants include GPU support (NVIDIA + AMD) — no separate per-GPU builds.

**Important:** When building from source, use `--features ebpf` for the agent to enable
eBPF-based state observation. Without this feature, eBPF probes are compiled out.

### Node Name Resolution

Pact uses DNS-based agent discovery: `pact exec <node-id>` resolves the node ID
to `http://<node-id>:9445`. Ensure node IDs are resolvable via DNS or `/etc/hosts`.

### Agent Auth Configuration

The agent shell server validates incoming OIDC tokens independently from the journal.
Configure `[agent.shell.auth]` in the agent TOML:

```toml
[agent.shell.auth]
issuer = "https://your-idp/realm"
audience = "pact-cli"
jwks_url = "https://your-idp/realm/protocol/openid-connect/certs"
# Optional: HMAC secret for dev/test (production uses JWKS only)
# hmac_secret = "shared-secret"
```

Without this section, the agent falls back to fail-closed (JWKS required, no secret).

### Prerequisites

Download release artifacts from [GitHub releases](https://github.com/witlox/pact/releases/latest).
You can create a provisioning bundle for easy distribution:

```bash
# Download release artifacts
mkdir -p /tmp/pact-release
gh release download v2026.1.196 --dir /tmp/pact-release \
    --pattern "pact-platform-x86_64.tar.gz" \
    --pattern "pact-agent-x86_64-pact.tar.gz"

# Create provisioning bundle (includes scripts + systemd units)
scripts/deploy/make-provision-bundle.sh /tmp/pact-release /tmp/pact-provision.tar.gz

# Upload to all nodes (single file, no scp --recurse issues)
scp /tmp/pact-provision.tar.gz node:/tmp/
ssh node 'cd /tmp && tar xzf pact-provision.tar.gz'
```

Or manually:
- Unpack binaries to `/opt/pact/bin/` on all nodes
- Copy `infra/systemd/` to `/opt/pact/systemd/` on all nodes
- Copy `scripts/deploy/` to `/opt/pact/deploy/` on all nodes

### Step 1: Create CA and distribute to all nodes

```bash
# On the first management node:
/opt/pact/deploy/setup-ca.sh /etc/pact/certs mgmt-1

# Then copy /etc/pact/ca/ to ALL other nodes (management + compute):
scp -r /etc/pact/ca/ mgmt-2:/etc/pact/ca/
scp -r /etc/pact/ca/ mgmt-3:/etc/pact/ca/
scp -r /etc/pact/ca/ compute-1:/etc/pact/ca/
# ...etc
```

### Step 2: Install journal on management nodes

```bash
# Peer format: id=addr (id matches node-id argument)
PEERS="1=mgmt-1:9443,2=mgmt-2:9443,3=mgmt-3:9443"

# Node 1 — with --bootstrap to initialize the Raft cluster
/opt/pact/deploy/install-management.sh 1 mgmt-1 "$PEERS" --bootstrap

# Nodes 2 and 3 — without --bootstrap (join existing cluster)
/opt/pact/deploy/install-management.sh 2 mgmt-2 "$PEERS"
/opt/pact/deploy/install-management.sh 3 mgmt-3 "$PEERS"

# Wait ~10 seconds for Raft membership replication, then verify:
/opt/pact/deploy/bootstrap-quorum.sh mgmt-1:9443
```

### Step 3: Install agent on compute nodes

```bash
JOURNALS="mgmt-1:9443,mgmt-2:9443,mgmt-3:9443"

/opt/pact/deploy/install-compute.sh compute-1 ml-training "$JOURNALS"
/opt/pact/deploy/install-compute.sh compute-2 ml-training "$JOURNALS"
```

### Step 4: Install monitoring (optional)

```bash
/opt/pact/deploy/install-monitoring.sh mgmt-1,mgmt-2,mgmt-3
```

### Step 5: Validate

```bash
# Run test matrix (v1=pact-only, v2=systemd, v3=pact+lattice, v4=systemd+lattice)
/opt/pact/deploy/validate.sh v1 mgmt-1:9443 compute-1,compute-2
```

---

## Manual Deployment (step-by-step)

## Journal Quorum Setup

The journal is pact's distributed immutable log, backed by a Raft consensus group.
Deploy it on dedicated management nodes or co-located with lattice (see ADR-001).

### Install the binary

Download the platform binaries for your architecture from the
[latest release](https://github.com/witlox/pact/releases/latest):

```bash
curl -LO https://github.com/witlox/pact/releases/latest/download/pact-platform-x86_64.tar.gz
tar xzf pact-platform-x86_64.tar.gz -C /usr/local/bin/
```

This installs `pact-journal`, `pact` (CLI), and `pact-mcp`.

### 3-Node Quorum (Standard)

A 3-node quorum tolerates 1 node failure. Suitable for most deployments.

Create `/etc/pact/journal.env` on each journal node:

**journal-1:**
```bash
PACT_JOURNAL_NODE_ID=1
PACT_JOURNAL_LISTEN=0.0.0.0:9443
PACT_JOURNAL_DATA_DIR=/var/lib/pact/journal
PACT_JOURNAL_PEERS=1=journal-1:9443,2=journal-2:9443,3=journal-3:9443
```

**journal-2:**
```bash
PACT_JOURNAL_NODE_ID=2
PACT_JOURNAL_LISTEN=0.0.0.0:9443
PACT_JOURNAL_DATA_DIR=/var/lib/pact/journal
PACT_JOURNAL_PEERS=1=journal-1:9443,2=journal-2:9443,3=journal-3:9443
```

**journal-3:**
```bash
PACT_JOURNAL_NODE_ID=3
PACT_JOURNAL_LISTEN=0.0.0.0:9443
PACT_JOURNAL_DATA_DIR=/var/lib/pact/journal
PACT_JOURNAL_PEERS=1=journal-1:9443,2=journal-2:9443,3=journal-3:9443
```

**Bootstrap:** On the first deploy, run `pact-journal --bootstrap` on node 1 to
initialize the Raft membership. The membership replicates to nodes 2 and 3
automatically within seconds. Do NOT use `--bootstrap` on subsequent restarts or
on nodes 2/3.

Create `/etc/pact/journal.toml` (same on all nodes, node ID comes from env):

```toml
[journal]
listen_addr = "0.0.0.0:9443"
data_dir = "/var/lib/pact/journal"

[journal.raft]
members = [
    "1:journal-1.mgmt:9444",
    "2:journal-2.mgmt:9444",
    "3:journal-3.mgmt:9444"
]
snapshot_interval = 10000

[journal.streaming]
max_concurrent_boot_streams = 15000

[policy]
enabled = true

[policy.iam]
oidc_issuer = "https://auth.example.org/realms/hpc"
oidc_audience = "pact"

[policy.engine]
type = "opa"
opa_endpoint = "http://localhost:8181/v1/data/pact"

[telemetry]
log_level = "info"
log_format = "json"
prometheus_enabled = true
prometheus_listen = "0.0.0.0:9091"
loki_enabled = true
loki_endpoint = "http://loki.mgmt:3100/loki/api/v1/push"
```

### 5-Node Quorum (High Availability)

A 5-node quorum tolerates 2 node failures. Recommended for large deployments
or when journal availability is critical (e.g., boot-time config streaming for
thousands of nodes).

Configuration is identical to 3-node, with two additional members:

```toml
[journal.raft]
members = [
    "1:journal-1.mgmt:9444",
    "2:journal-2.mgmt:9444",
    "3:journal-3.mgmt:9444",
    "4:journal-4.mgmt:9444",
    "5:journal-5.mgmt:9444"
]
```

### Co-Located with Lattice

If running on the same management nodes as lattice, use separate ports and data
directories. pact is the incumbent: journal quorum starts before lattice.

| Component | Raft Port | gRPC Port | Data Dir |
|-----------|-----------|-----------|----------|
| pact-journal | 9444 | 9443 | `/var/lib/pact/journal` |
| lattice-server | 9000 | 50051 | `/var/lib/lattice/raft` |

**Lattice bootstrap:** Like pact-journal, lattice-server requires `--bootstrap` on first
start of node 1 to initialize the Raft cluster. Subsequent restarts must NOT use
`--bootstrap`. See lattice deployment guide for details.

**Lattice agent:** Deploy `lattice-agent` on compute nodes. It registers with the
lattice scheduler via heartbeats. Pact's supercharged commands (`drain`, `cordon`,
`uncordon`) delegate to lattice — they require the node to be registered in both
pact (enrollment) and lattice (agent heartbeat).

```bash
# Set delegation endpoint so pact CLI can reach lattice
export PACT_LATTICE_ENDPOINT=http://mgmt-1:50051
pact drain compute-1    # → delegates to lattice
```

**Known limitations (pending lattice fixes):**
- Drain state machine: node may stay in `Draining` instead of transitioning to
  `Drained` when allocations reach 0. Undrain only works from `Drained` state.
- Auth: lattice REST/gRPC does not enforce authentication when `oidc_issuer` is
  empty. Configure lattice with the same IdP as pact for production.
- Process reattach: lattice-agent restart kills running workloads. Use
  `KillMode=process` in systemd to avoid killing children, but the agent
  cannot yet reattach to orphaned processes after restart.

### Port Summary

| Port | Service | Protocol |
|------|---------|----------|
| 9443 | pact-journal gRPC | gRPC (config, streaming) |
| 9444 | pact-journal Raft | Raft consensus |
| 9445 | pact-agent shell/exec | gRPC |
| 9091 | pact-journal metrics | HTTP (Prometheus) |

## Agent Installation

### Install the binary

Download the agent variant for your architecture and supervisor mode from the
[latest release](https://github.com/witlox/pact/releases/latest):

```bash
# PactSupervisor mode (PID 1 / diskless HPC) — includes NVIDIA + AMD GPU support
curl -LO https://github.com/witlox/pact/releases/latest/download/pact-agent-x86_64-pact.tar.gz
tar xzf pact-agent-x86_64-pact.tar.gz
sudo mv pact-agent /usr/local/bin/

# Or systemd mode (traditional service)
curl -LO https://github.com/witlox/pact/releases/latest/download/pact-agent-x86_64-systemd.tar.gz
tar xzf pact-agent-x86_64-systemd.tar.gz
sudo mv pact-agent /usr/local/bin/
```

For diskless nodes, include the `pact-agent` binary in the base SquashFS image
provisioned by OpenCHAMI.

### Create the config

Create `/etc/pact/agent.toml`:

```toml
[agent]
enforcement_mode = "enforce"

[agent.supervisor]
backend = "pact"

[agent.journal]
endpoints = ["journal-1.mgmt:9443", "journal-2.mgmt:9443", "journal-3.mgmt:9443"]
tls_enabled = true
tls_cert = "/etc/pact/agent.crt"
tls_key = "/etc/pact/agent.key"
tls_ca = "/etc/pact/ca.crt"

[agent.observer]
ebpf_enabled = true
inotify_enabled = true
netlink_enabled = true

[agent.shell]
enabled = true
listen = "0.0.0.0:9445"
whitelist_mode = "strict"

[agent.commit_window]
base_window_seconds = 900
drift_sensitivity = 2.0
emergency_window_seconds = 14400

[agent.blacklist]
patterns = ["/tmp/**", "/var/log/**", "/proc/**", "/sys/**", "/dev/**",
            "/run/user/**", "/run/pact/**", "/run/lattice/**"]
```

### Node identity

The agent's `node_id` is typically set via environment variable or auto-detected
from the hostname. For diskless nodes, OpenCHAMI sets the hostname during PXE boot.

## Identity and mTLS Setup

pact uses mutual TLS for agent-to-journal communication. Identity is provisioned
automatically -- no manual certificate generation is needed for agents or journal nodes.

### SPIRE (primary, when deployed)

When SPIRE is deployed at the site, pact uses it as the primary identity provider.
Agents and journal nodes receive SPIFFE SVIDs (X.509 certificates) via SPIRE node
attestation. SPIRE handles certificate rotation automatically.

Configure the SPIRE agent socket in the agent config:

```toml
[agent.identity]
provider = "spire"
spire_socket = "/run/spire/agent.sock"
```

### Ephemeral CA (fallback, default)

When SPIRE is not available, the journal quorum generates an ephemeral CA at startup.
Agents enroll via the CSR workflow -- no manual certificate provisioning is required.

**Enrollment workflow:**

1. Platform admin enrolls a node: `pact enroll <node> --hardware-id <hw-id>`
2. The journal records the enrollment with the node's expected hardware identity
3. Agent boots and presents its hardware identity (TPM, SMBIOS UUID, or MAC-based)
4. Agent generates a keypair and submits a CSR to the journal
5. Journal validates the hardware identity against the enrollment record
6. Journal signs the CSR with the ephemeral CA and returns the certificate
7. Agent uses the signed certificate for mTLS from this point forward

The ephemeral CA is regenerated when the journal quorum restarts. Agents automatically
re-enroll to obtain new certificates.

### CA cert distribution

Agents need the CA certificate bundle to validate journal server certificates.
For diskless nodes, include the CA cert in the base SquashFS image:

- `/etc/pact/ca.crt` -- CA certificate bundle (all nodes)

For SPIRE deployments, the SPIRE trust bundle replaces this file. For ephemeral CA
deployments, the journal serves the CA cert during the enrollment handshake.

### Identity mapping

In PactSupervisor mode, identity mapping (pact-nss) is automatic -- the agent
maps SPIFFE IDs or certificate CNs to local UIDs without manual NSS configuration.

## OIDC Provider Configuration

pact authenticates admins via OIDC tokens. Configure your identity provider
(Keycloak, Auth0, Okta, etc.) with the following:

### Create a pact client

- **Client ID**: `pact`
- **Client type**: Public (CLI) or Confidential (MCP server)
- **Redirect URI**: `http://localhost:8400/callback` (for CLI login flow)

### Define roles

Create these roles in your OIDC provider and assign them to users:

| Role | Description |
|------|-------------|
| `pact-platform-admin` | Full system access (2-3 people per site) |
| `pact-ops-{vcluster}` | Day-to-day ops for a vCluster |
| `pact-viewer-{vcluster}` | Read-only access |
| `pact-regulated-{vcluster}` | Ops with two-person approval |
| `pact-service-agent` | Machine identity for agents (mTLS) |
| `pact-service-ai` | Machine identity for MCP server |

### Configure the journal

Set the OIDC issuer and audience in the journal config:

```toml
[policy.iam]
oidc_issuer = "https://auth.example.org/realms/hpc"
oidc_audience = "pact"
```

## systemd Service Management

### Install systemd units

Copy the provided unit files:

```bash
cp infra/systemd/pact-journal.service /etc/systemd/system/
cp infra/systemd/pact-agent.service /etc/systemd/system/
```

### Enable and start

**Journal nodes:**
```bash
systemctl daemon-reload
systemctl enable pact-journal
systemctl start pact-journal
```

**Compute nodes:**
```bash
systemctl daemon-reload
systemctl enable pact-agent
systemctl start pact-agent
```

### Check status

```bash
systemctl status pact-journal
journalctl -u pact-journal -f
```

### Environment files

The systemd units read environment variables from:
- `/etc/pact/journal.env` (journal nodes)
- `/etc/pact/agent.env` (compute nodes)

## Docker Compose Deployment

For development, testing, or small deployments, use the provided Docker Compose
configuration.

```bash
cd infra/docker
docker compose up -d
```

This starts:

| Service | Container | Ports |
|---------|-----------|-------|
| journal-1 | pact-journal-1 | 9443, 9091 |
| journal-2 | pact-journal-2 | 9543, 9191 |
| journal-3 | pact-journal-3 | 9643, 9291 |
| agent | pact-agent | 9445 |
| prometheus | pact-prometheus | 9090 |
| grafana | pact-grafana | 3000 |

Access Grafana at `http://localhost:3000` (login: admin / admin).

### Scaling

To run multiple agents in Docker:

```bash
docker compose up -d --scale agent=5
```

## Monitoring with Grafana + Prometheus

### Prometheus

pact-journal exposes Prometheus metrics on the metrics listen port (default 9091).
The provided `infra/docker/prometheus.yml` scrapes all journal nodes:

```yaml
scrape_configs:
  - job_name: "pact-journal"
    static_configs:
      - targets:
          - "journal-1:9091"
          - "journal-2:9091"
          - "journal-3:9091"
```

### Loki

pact-journal streams events to Loki for structured log aggregation. Configure
the Loki endpoint in journal config:

```toml
[telemetry]
loki_enabled = true
loki_endpoint = "http://loki.mgmt:3100/loki/api/v1/push"
```

### Grafana dashboards

Import the dashboards from `infra/grafana/dashboards/` into Grafana. These provide:

- Journal quorum health (Raft leader, commit index, log size)
- Node status overview (drift, services, capabilities)
- Config change timeline
- Emergency mode events
- Approval workflow status

### Alerting

Import the alerting rules from `infra/alerting/rules.yml` into Prometheus. Key alerts:

- Raft leader election timeout
- Journal node down
- Agent disconnected
- Emergency mode entered
- Pending approvals nearing expiry
- Excessive drift on node

## OPA Policy Engine

pact uses OPA (Open Policy Agent) for authorization decisions. Deploy OPA as a
sidecar on each journal node.

### Install OPA

```bash
# Download OPA binary
curl -L -o /usr/local/bin/opa \
    https://openpolicyagent.org/downloads/v0.73.0/opa_linux_amd64_static
chmod +x /usr/local/bin/opa
```

### Run OPA

```bash
opa run --server --addr localhost:8181 /etc/pact/policies/
```

### Configure journal

```toml
[policy.engine]
type = "opa"
opa_endpoint = "http://localhost:8181/v1/data/pact"
```

### Policy federation

If using Sovra for cross-site federation, policy templates are synchronized
automatically:

```toml
[policy.federation]
sovra_endpoint = "https://sovra.mgmt:8443"
sync_interval_seconds = 300
```
