# Building an OpenCHAMI Image with pact

This guide covers building a diskless SquashFS compute node image with pact-agent
as the init system (PID 1), SPIRE for workload identity, and OpenCHAMI for
boot provisioning.

## Overview

The boot chain on a diskless HPC node:

```
BMC/PXE → OpenCHAMI DHCP → iPXE → kernel + initramfs
  → mount SquashFS root (read-only)
  → pivot_root
  → pact-agent starts as PID 1
  → authenticates to journal (SPIRE or bootstrap cert)
  → streams vCluster config overlay
  → applies config (sysctl, modules, mounts, uenv)
  → starts services in dependency order
  → reports capabilities → node ready
```

## What Goes in the Image vs What Gets Streamed

| In the SquashFS image (static) | Streamed at boot (dynamic) |
|--------------------------------|---------------------------|
| pact-agent binary | vCluster overlay (sysctl, modules, mounts) |
| SPIRE agent binary + config | Node-specific delta (per-node tunables) |
| Bootstrap CA cert (`/etc/pact/ca.crt`) | Service declarations (what to start) |
| Base OS packages (glibc, coreutils, etc.) | OPA policy bundles |
| GPU drivers (NVIDIA/AMD) | Identity (SVID via SPIRE or CSR) |
| Network drivers (cxi, i40e, etc.) | |
| pact agent config (`/etc/pact/agent.toml`) | |

The image is read-only. All runtime state goes to tmpfs (`/run/pact/`, `/tmp/`).

## Prerequisites

- [OpenCHAMI](https://openchami.org) deployed (SMD, BSS, DHCP, image server)
- A build host with `mksquashfs`, `debootstrap` (or equivalent), and the pact
  release binaries
- SPIRE server running on management nodes (optional but recommended)
- pact-journal quorum running on management nodes

## Step 1: Create the Base Root Filesystem

Start with a minimal Linux root. The exact method depends on your distro:

```bash
# Ubuntu/Debian
mkdir -p /tmp/pact-image/rootfs
sudo debootstrap --variant=minbase noble /tmp/pact-image/rootfs http://archive.ubuntu.com/ubuntu

# Or SUSE (for Cray/HPE systems)
# zypper --root /tmp/pact-image/rootfs install ...

# Or from an existing node image
# rsync -a /path/to/base-image/ /tmp/pact-image/rootfs/
```

Install essential packages in the chroot:

```bash
sudo chroot /tmp/pact-image/rootfs /bin/bash -c '
    apt-get update
    apt-get install -y --no-install-recommends \
        ca-certificates \
        iproute2 \
        kmod \
        procps \
        util-linux \
        chrony
'
```

## Step 2: Install pact-agent

Download the agent binary matching your target hardware:

```bash
# Example: x86_64 NVIDIA with PactSupervisor
curl -LO https://github.com/witlox/pact/releases/latest/download/pact-agent-x86_64-nvidia-pact.tar.gz
sudo tar xzf pact-agent-x86_64-nvidia-pact.tar.gz -C /tmp/pact-image/rootfs/usr/local/bin/
```

## Step 3: Install SPIRE Agent

SPIRE provides workload identity (X.509 SVIDs) for mTLS between pact-agent and
the journal. If SPIRE is not available, pact falls back to the bootstrap
certificate + ephemeral CA workflow.

```bash
# Download SPIRE agent
SPIRE_VERSION=1.12.0
curl -LO https://github.com/spiffe/spire/releases/download/v${SPIRE_VERSION}/spire-${SPIRE_VERSION}-linux-amd64-musl.tar.gz
tar xzf spire-${SPIRE_VERSION}-linux-amd64-musl.tar.gz
sudo cp spire-${SPIRE_VERSION}/bin/spire-agent /tmp/pact-image/rootfs/usr/local/bin/
```

Create the SPIRE agent config:

```bash
sudo mkdir -p /tmp/pact-image/rootfs/etc/spire
sudo tee /tmp/pact-image/rootfs/etc/spire/agent.conf << 'EOF'
agent {
    data_dir = "/run/spire/agent"
    log_level = "INFO"
    server_address = "spire-server.mgmt"
    server_port = "8081"
    socket_path = "/run/spire/agent.sock"
    trust_domain = "example.org"

    # Node attestation via TPM or join token
    NodeAttestor "tpm_devid" {
        plugin_data {}
    }
}
EOF
```

For sites without TPM, use join token attestation instead:

```bash
# On the SPIRE server, create a join token for this node class:
#   spire-server token generate -spiffeID spiffe://example.org/pact-agent
# Then inject the token into the image or pass via kernel cmdline.
```

## Step 4: Install GPU Drivers

For NVIDIA nodes:

```bash
# Install NVIDIA driver + persistenced (in chroot)
sudo chroot /tmp/pact-image/rootfs /bin/bash -c '
    # Install from your driver repo or CUDA toolkit
    apt-get install -y nvidia-driver-570 nvidia-utils-570
'
```

For AMD nodes:

```bash
sudo chroot /tmp/pact-image/rootfs /bin/bash -c '
    # Install ROCm driver
    apt-get install -y rocm-smi-lib
'
```

## Step 5: Install Network Drivers

For Slingshot (Cray CXI) fabric:

```bash
# CXI drivers are typically provided by HPE/Cray as RPMs or DEBs
# Install cxi-driver, cxi-utils, libfabric-cxi
sudo chroot /tmp/pact-image/rootfs /bin/bash -c '
    dpkg -i /path/to/cxi-driver_*.deb
'
```

## Step 6: Configure pact-agent

Create the agent config. The `node_id` and `vcluster` are set dynamically
at boot via environment variables (OpenCHAMI sets the hostname):

```bash
sudo mkdir -p /tmp/pact-image/rootfs/etc/pact
sudo tee /tmp/pact-image/rootfs/etc/pact/agent.toml << 'EOF'
[agent]
# node_id auto-detected from hostname (set by OpenCHAMI DHCP)
enforcement_mode = "enforce"

[agent.supervisor]
backend = "pact"

[agent.journal]
endpoints = [
    "journal-1.mgmt:9443",
    "journal-2.mgmt:9443",
    "journal-3.mgmt:9443",
]
tls_enabled = true
tls_ca = "/etc/pact/ca.crt"

[agent.identity]
provider = "spire"
spire_socket = "/run/spire/agent.sock"

[agent.observer]
ebpf_enabled = true
inotify_enabled = true
netlink_enabled = true

[agent.shell]
enabled = true
listen = "0.0.0.0:9445"
whitelist_mode = "strict"

[agent.capability]
manifest_path = "/run/pact/capability.json"
socket_path = "/run/pact/capability.sock"
gpu_poll_interval_seconds = 30

[agent.commit_window]
base_window_seconds = 900
drift_sensitivity = 2.0
emergency_window_seconds = 14400

[agent.blacklist]
patterns = [
    "/tmp/**", "/var/log/**", "/proc/**", "/sys/**",
    "/dev/**", "/run/user/**", "/run/pact/**", "/run/lattice/**",
]
EOF
```

## Step 7: Configure pact-agent as PID 1

Create an init wrapper that sets up minimal infrastructure before handing off
to pact-agent. The SquashFS root is read-only, so we need tmpfs mounts:

```bash
sudo tee /tmp/pact-image/rootfs/init << 'INITEOF'
#!/bin/sh
# Minimal init for pact-agent as PID 1 on diskless nodes.
# Called directly by the kernel after pivot_root.

# Mount essential filesystems
mount -t proc proc /proc
mount -t sysfs sysfs /sys
mount -t devtmpfs devtmpfs /dev
mount -t tmpfs tmpfs /run
mount -t tmpfs tmpfs /tmp
mkdir -p /run/pact /run/spire/agent /run/lock /var/log

# Load essential modules
modprobe -a overlay tmpfs

# Set hostname from kernel cmdline (OpenCHAMI sets pact.nodeid=)
NODEID=$(sed -n 's/.*pact.nodeid=\([^ ]*\).*/\1/p' /proc/cmdline)
[ -n "$NODEID" ] && hostname "$NODEID"

# Start SPIRE agent in background (if available)
if [ -x /usr/local/bin/spire-agent ]; then
    /usr/local/bin/spire-agent run \
        -config /etc/spire/agent.conf \
        -logLevel INFO &
    # Give SPIRE a moment to create the socket
    sleep 1
fi

# Hand off to pact-agent
exec /usr/local/bin/pact-agent --config /etc/pact/agent.toml
INITEOF
sudo chmod +x /tmp/pact-image/rootfs/init
```

## Step 8: Install Bootstrap CA Certificate

For the initial boot before SPIRE is available, include the journal's CA cert:

```bash
# Copy from a journal node or generate during journal setup
sudo cp /etc/pact/ca.crt /tmp/pact-image/rootfs/etc/pact/ca.crt
```

If using SPIRE exclusively, this cert is only needed for the first connection
to obtain the SPIRE join token or for fallback when SPIRE is unavailable.

## Step 9: Build the SquashFS Image

```bash
sudo mksquashfs /tmp/pact-image/rootfs /tmp/pact-image/pact-node.squashfs \
    -comp zstd \
    -Xcompression-level 19 \
    -noappend \
    -no-recovery \
    -processors $(nproc)
```

Typical image sizes:
- Base + pact-agent + SPIRE: ~300 MB
- With NVIDIA drivers: ~800 MB
- With ROCm: ~600 MB

## Step 10: Register with OpenCHAMI

Upload the image to OpenCHAMI's image server and configure the boot parameters:

```bash
# Upload image to OpenCHAMI image server.
# Use your site's image management tooling to upload the SquashFS to the image server,
# e.g. scp, s3 upload, or your image registry workflow.

# Set boot parameters for a node group via BSS REST API
curl -X PUT https://bss.mgmt/boot/v1/bootparameters \
    -H "Content-Type: application/json" \
    -d '{
        "macs": [],
        "hosts": ["ml-training"],
        "params": "root=live:http://image-server/pact-ml-training-v1.squashfs init=/init pact.nodeid=${hostname} console=tty0",
        "kernel": "http://image-server/vmlinuz",
        "initrd": "http://image-server/initramfs.img"
    }'
```

The `init=/init` parameter tells the kernel to run our init wrapper.
The `pact.nodeid=${hostname}` is expanded by OpenCHAMI's DHCP/BSS.

## Step 11: Pre-enroll Nodes

Before the first boot, register nodes in the journal:

```bash
# Enroll nodes with their hardware identity
pact node enroll compute-001 --mac aa:bb:cc:dd:ee:01
pact node enroll compute-002 --mac aa:bb:cc:dd:ee:02
# ... or batch import from SMD inventory:
pact node import --group ml-training

# Assign to vCluster
pact node assign compute-001 --vcluster ml-training
pact node assign compute-002 --vcluster ml-training
```

## Step 12: Boot and Verify

Power on the nodes via OpenCHAMI/Redfish:

```bash
# Power on nodes via BMC/Redfish (use your BMC management tool: ipmitool, Redfish, etc.)
# Example with curl against OpenCHAMI SMD:
curl -X POST https://smd.mgmt/hsm/v2/State/Components/x1000c0s0b0n0/Actions/PowerCycle \
    -H "Content-Type: application/json" \
    -d '{"ResetType": "On"}'
# Or use pact's delegation command for enrolled nodes:
pact reboot compute-001
```

Monitor boot progress:

```bash
# Watch the journal for enrollment events
pact watch --vcluster ml-training

# Check node status (should appear within ~2 seconds of boot)
pact status --vcluster ml-training

# Verify capabilities
pact cap compute-001

# Check service status
pact service status compute-001
```

## Updating the Image

To update the base image (new drivers, new pact-agent version):

1. Build a new SquashFS image (steps 1-9)
2. Upload to OpenCHAMI image server (using your site's image management tooling)
3. Update boot config via BSS REST API: `curl -X PUT https://bss.mgmt/boot/v1/bootparameters -d '{"hosts":["ml-training"],"params":"root=live:http://image-server/pact-ml-training-v2.squashfs ..."}'`
4. Rolling reboot: `pact drain compute-001 && pact reboot compute-001`

Nodes pick up the new image on reboot. pact configuration (sysctl, mounts,
services) is streamed from the journal — not baked into the image — so most
config changes don't require a new image.

## Including Lattice (Supercharged Mode)

When deploying pact alongside lattice for workload scheduling, the compute node
image includes both `pact-agent` and `lattice-node-agent`. pact supervises
lattice-node-agent as a declared service — this is "supercharged mode" where
both systems cooperate.

### Additional binaries in the image

Add lattice-node-agent to the SquashFS image alongside pact-agent:

```bash
# Download lattice node agent
curl -LO https://github.com/witlox/lattice/releases/latest/download/lattice-node-agent-x86_64.tar.gz
sudo tar xzf lattice-node-agent-x86_64.tar.gz -C /tmp/pact-image/rootfs/usr/local/bin/
```

### lattice-node-agent config

Create the lattice node agent config. The agent connects to the lattice
scheduler quorum and reports node capabilities (read from pact's capability
manifest):

```bash
sudo mkdir -p /tmp/pact-image/rootfs/etc/lattice
sudo tee /tmp/pact-image/rootfs/etc/lattice/node-agent.toml << 'EOF'
[node_agent]
# lattice scheduler quorum endpoints (on HSN, not management network — ADR-017)
scheduler_endpoints = [
    "lattice-1.hsn:50051",
    "lattice-2.hsn:50051",
    "lattice-3.hsn:50051",
]

# pact capability manifest (lattice-node-agent reads this)
capability_manifest = "/run/pact/capability.json"
capability_socket = "/run/pact/capability.sock"

# Namespace handoff socket (pact creates namespaces, lattice uses them)
namespace_socket = "/run/pact/ns-handoff.sock"

# Mount refcounting (shared between pact and lattice)
mount_socket = "/run/pact/mount-refcount.sock"

[node_agent.identity]
# Uses the same SPIRE socket as pact for workload identity
spire_socket = "/run/spire/agent.sock"
EOF
```

### Declare lattice-node-agent as a pact service

pact-agent supervises lattice-node-agent as a declared service. This is
configured in the vCluster overlay (streamed at boot, not baked in the image).

Create the overlay spec:

```toml
# vcluster-overlay.toml — applied with: pact apply vcluster-overlay.toml
[vcluster.ml-training.services.lattice-node-agent]
binary = "/usr/local/bin/lattice-node-agent"
args = ["--config", "/etc/lattice/node-agent.toml"]
restart_policy = "always"
order = 50
depends_on = ["chronyd"]

[vcluster.ml-training.services.chronyd]
binary = "/usr/sbin/chronyd"
args = ["-d"]
restart_policy = "always"
order = 10

# For GPU nodes, add nvidia-persistenced
[vcluster.ml-training.services.nvidia-persistenced]
binary = "/usr/bin/nvidia-persistenced"
args = ["--no-persistence-mode"]
restart_policy = "on_failure"
order = 20
```

### Boot sequence with lattice

```
Kernel → SquashFS root → pact-agent (PID 1)
  → auth to journal → stream vCluster config overlay
  → apply: kernel params, modules, mounts, uenv
  → start services in dependency order:
      1. chronyd (time sync)
      2. nvidia-persistenced (GPU, if declared)
      3. lattice-node-agent (workload scheduling)
  → pact writes CapabilityReport to /run/pact/capability.json
  → lattice-node-agent reads manifest, reports to scheduler
  → node ready for workloads
```

### Supercharged CLI

With both systems running, operators get unified admin access:

```bash
# pact-native commands work as before
pact status --vcluster ml-training
pact exec compute-001 -- nvidia-smi
pact diag compute-001 --grep "ECC"

# Supercharged commands query both systems
pact jobs list --vcluster ml-training    # lattice allocations
pact health                              # pact + lattice health
pact drain compute-001                   # lattice drain + pact audit
```

Configure the lattice endpoint for supercharged commands. Note: the pact CLI
connects to lattice's HSN-facing gRPC port from the admin workstation (which
must have HSN access or a management-to-HSN gateway):

```bash
export PACT_LATTICE_ENDPOINT=http://lattice-1.hsn:50051
export PACT_LATTICE_TOKEN=<lattice-auth-token>
```

### Network separation

pact and lattice run on separate networks (ADR-017). pact uses the management
network exclusively. Lattice runs entirely on the HSN — including agent↔scheduler
communication, Raft consensus, and workload data.

| Traffic | Network | Port |
|---------|---------|------|
| pact agent ↔ journal | Management | 9443, 9444 |
| pact shell/exec/diag | Management | 9445 |
| pact journal metrics | Management | 9091 |
| lattice agent ↔ scheduler | **HSN** | 50051 |
| lattice Raft consensus | **HSN** | 9000 |
| Workload data (MPI, NCCL) | **HSN** | Application-defined |

pact never touches the HSN. Lattice never touches pact's management ports.
If the HSN goes down, pact continues operating (admin access, config management)
while lattice pauses scheduling. If the management network goes down, pact
agents use cached config while lattice is unaffected.

## Troubleshooting

### Node doesn't appear after boot

```bash
# Check if the node enrolled
pact node list --vcluster ml-training

# Check journal logs for enrollment errors
pact audit --source pact -n 20

# If node is reachable via BMC console:
#   - Check /run/pact/ for agent logs
#   - Check if SPIRE socket exists: ls /run/spire/agent.sock
#   - Check if journal is reachable: curl -k https://journal-1.mgmt:9443/health
```

### SPIRE agent fails to attest

```bash
# On the SPIRE server, check registration entries:
spire-server entry show

# Create a join token for manual attestation:
spire-server token generate -spiffeID spiffe://example.org/pact-agent/compute-001

# Pass the token to the node via kernel cmdline (update BSS):
#   curl -X PUT https://bss.mgmt/boot/v1/bootparameters \
#     -d '{"hosts":["compute-001"],"params":"... spire.join_token=<token>"}'
```

### Agent falls back to bootstrap identity

This is normal on first boot or when SPIRE is unavailable. The agent will:
1. Use the bootstrap CA cert for initial journal connection
2. Submit a CSR to the journal
3. Journal validates hardware identity and signs the cert
4. Agent switches to the journal-signed cert

Once SPIRE becomes available, the agent rotates to SPIRE-managed mTLS
automatically (identity cascade: SPIRE → journal-signed → bootstrap).
