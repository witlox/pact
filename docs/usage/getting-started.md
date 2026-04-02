# Getting Started with pact

## Install from Release

Download pre-built binaries from the [latest release](https://github.com/witlox/pact/releases/latest).

### Platform binaries (journal, CLI, MCP)

```bash
# x86_64
curl -LO https://github.com/witlox/pact/releases/latest/download/pact-platform-x86_64.tar.gz
sudo tar xzf pact-platform-x86_64.tar.gz -C /usr/local/bin/

# aarch64
curl -LO https://github.com/witlox/pact/releases/latest/download/pact-platform-aarch64.tar.gz
sudo tar xzf pact-platform-aarch64.tar.gz -C /usr/local/bin/
```

This installs `pact` (CLI), `pact-journal`, and `pact-mcp`.

### Agent binary

Choose the variant matching your hardware and supervisor model:

```bash
# x86_64 NVIDIA node with PactSupervisor (diskless HPC)
curl -LO https://github.com/witlox/pact/releases/latest/download/pact-agent-x86_64-nvidia-pact.tar.gz
sudo tar xzf pact-agent-x86_64-nvidia-pact.tar.gz -C /usr/local/bin/

# aarch64 NVIDIA node with PactSupervisor
curl -LO https://github.com/witlox/pact/releases/latest/download/pact-agent-aarch64-nvidia-pact.tar.gz
sudo tar xzf pact-agent-aarch64-nvidia-pact.tar.gz -C /usr/local/bin/
```

Available agent variants:

| Variant | Arch | GPU | Supervisor |
|---------|------|-----|------------|
| `pact-agent-x86_64-pact` | x86_64 | — | PactSupervisor |
| `pact-agent-x86_64-nvidia-pact` | x86_64 | NVIDIA | PactSupervisor |
| `pact-agent-x86_64-amd-pact` | x86_64 | AMD | PactSupervisor |
| `pact-agent-x86_64-systemd` | x86_64 | — | systemd |
| `pact-agent-x86_64-nvidia-systemd` | x86_64 | NVIDIA | systemd |
| `pact-agent-x86_64-amd-systemd` | x86_64 | AMD | systemd |
| `pact-agent-aarch64-pact` | aarch64 | — | PactSupervisor |
| `pact-agent-aarch64-nvidia-pact` | aarch64 | NVIDIA | PactSupervisor |
| `pact-agent-aarch64-systemd` | aarch64 | — | systemd |
| `pact-agent-aarch64-nvidia-systemd` | aarch64 | NVIDIA | systemd |

All variants include eBPF and SPIRE support. See [ARCHITECTURE.md](../../ARCHITECTURE.md#feature-flags) for details.

### Verify

```bash
pact --version
pact-agent --version
pact-journal --version
```

## Build from Source (development)

### Prerequisites

- **Rust toolchain**: stable (1.85+). The repo pins the toolchain via `rust-toolchain.toml`.
- **protoc**: Protocol Buffers compiler (required for building `pact-common`).
  - macOS: `brew install protobuf`
  - Ubuntu/Debian: `apt install protobuf-compiler`
- **just**: Task runner. Install with `cargo install just`.
- **Docker**: Required only for e2e tests and docker-compose deployment.
- **cargo-nextest** (optional, recommended): Faster test runner. Install with `cargo install cargo-nextest`.
- **cargo-deny** (optional): License and advisory checks. Install with `cargo install cargo-deny`.

Clone and build the entire workspace:

```bash
git clone https://github.com/witlox/pact.git
cd pact
cargo build --workspace
```

This produces four binaries in `target/debug/`:
- `pact` -- CLI tool
- `pact-agent` -- per-node daemon
- `pact-journal` -- distributed log server (Raft quorum member)
- `pact-mcp` -- AI agent tool-use interface

For optimized release builds:

```bash
just release
# Binaries in target/release/
```

## Running a Dev Cluster

### 1. Start the journal

The journal is the central log. For development, a single-node journal is sufficient:

```bash
just run-journal
```

This runs `pact-journal` with `config/minimal.toml`, listening on `localhost:9443`.

### 2. Start an agent

In a second terminal:

```bash
just run-agent
```

This runs `pact-agent` with `config/minimal.toml`. The agent connects to the journal
at `localhost:9443` and starts the shell server on port `9445`.

### 3. Use the CLI

In a third terminal, run commands against the local journal:

```bash
# Check node status
just cli status

# View configuration log (last 10 entries)
just cli log -n 10

# Commit current drift with a message
just cli commit -m "initial dev setup"
```

You can also run the CLI binary directly:

```bash
cargo run --package pact-cli -- status
cargo run --package pact-cli -- log -n 5
```

## First CLI Commands

### Check status

```bash
pact status                    # All nodes in default vCluster
pact status dev-node-001       # Specific node
```

### View configuration log

```bash
pact log                       # Last 20 entries (default)
pact log -n 50                 # Last 50 entries
pact log --scope node:dev-001  # Filter by node
```

### Show drift (declared vs actual)

```bash
pact diff                      # Current node
pact diff dev-node-001         # Specific node
```

### Commit drift

```bash
pact commit -m "tuned hugepages for training workload"
```

### Roll back

```bash
pact rollback 42               # Roll back to sequence number 42
```

## Configuration Basics

pact uses TOML configuration files. There are two main configs:

### Agent config (`agent.toml`)

Controls the per-node daemon. Key sections:

```toml
[agent]
node_id = "dev-node-001"
vcluster = "dev-sandbox"
enforcement_mode = "observe"    # "observe" | "enforce"

[agent.supervisor]
backend = "pact"                # "pact" (built-in) | "systemd" (fallback)

[agent.journal]
endpoints = ["localhost:9443"]
tls_enabled = false

[agent.observer]
ebpf_enabled = false
inotify_enabled = true
netlink_enabled = true

[agent.shell]
enabled = true
listen = "0.0.0.0:9445"
whitelist_mode = "learning"     # "strict" | "learning" | "bypass"

[agent.commit_window]
base_window_seconds = 900
```

See `config/minimal.toml` for a complete development config and
`config/production.toml` for a production-ready example.

### CLI config (`~/.config/pact/cli.toml`)

Controls the CLI tool. Created automatically on first use if missing:

```toml
endpoint = "http://localhost:9443"
default_vcluster = "dev-sandbox"
output_format = "text"          # "text" | "json"
timeout_seconds = 30
```

The CLI resolves settings with this precedence (highest to lowest):
1. Command-line flags (`--endpoint`, `--token`, `--vcluster`)
2. Environment variables (`PACT_ENDPOINT`, `PACT_TOKEN`, `PACT_VCLUSTER`)
3. Config file (`~/.config/pact/cli.toml`)
4. Defaults (`http://localhost:9443`)

### Authentication

For development, no token is needed if the journal has policy disabled
(`[policy] enabled = false` in `config/minimal.toml`).

For production, set your OIDC token:

```bash
# Via environment variable
export PACT_TOKEN="eyJhbGciOiJS..."

# Via token file
echo "eyJhbGciOiJS..." > ~/.config/pact/token

# Via CLI flag
pact --token "eyJhbGciOiJS..." status
```

## Running Tests

```bash
just test            # Unit + integration tests (fast, no Docker needed)
just test-accept     # BDD acceptance tests (584 scenarios)
just test-e2e        # End-to-end tests (requires Docker)
just ci              # Full CI suite (fmt + clippy + tests + deny)
```

## Docker Compose (Full Stack)

For a complete local environment with monitoring:

```bash
cd infra/docker
docker compose up -d
```

This starts:
- 3-node journal quorum (ports 9443, 9543, 9643)
- 1 agent (port 9445)
- Prometheus (port 9090)
- Grafana (port 3000, login: admin/admin)

## Next Steps

- [CLI Reference](cli-reference.md) -- all commands with detailed options
- [Admin Operations](admin-operations.md) -- day-to-day operational workflows
- [Deployment Guide](deployment.md) -- production deployment
- [Troubleshooting](troubleshooting.md) -- common issues and solutions
