# ADR-017: Network Topology — Management Network for Pact, HSN for Lattice

## Status: Accepted

## Context

HPC infrastructure has two distinct networks:

- **Management network** (1G Ethernet): OpenCHAMI, BMC/IPMI, PXE boot, admin access.
  Always available. Low bandwidth, high reliability.
- **High-speed network** (Slingshot/Ultra Ethernet, 200G+): workload traffic, MPI/NCCL,
  storage data plane. High bandwidth, low latency. Requires `cxi_rh` (Slingshot resource
  handler) to be running.

Both pact and lattice need mTLS-authenticated gRPC communication. The question: which
network carries which traffic?

## Decision

**Pact traffic runs entirely on the management network. Lattice traffic runs on the
high-speed network (HSN). SPIRE provides network-agnostic identity to both.**

### Pact on management network

| Traffic | Direction | Size | Frequency |
|---------|-----------|------|-----------|
| Enrollment (CSR + cert) | Agent → Journal | ~5 KB | Once per boot |
| Boot overlay streaming | Journal → Agent | 100-200 KB (zstd) | Once per boot |
| Node delta | Journal → Agent | <1 KB | Once per boot |
| Config subscription | Journal → Agent | Events (bytes) | Occasional |
| Heartbeat (stream keepalive) | Agent ↔ Journal | Bytes | Continuous |
| Exec/shell (interactive) | CLI → Agent | Variable | On demand |
| Audit events | Agent → Journal | ~1 KB each | Per operation |
| Journal Raft consensus | Journal ↔ Journal | Config entries | On writes |

Journal listens on management network:
- gRPC: port 9443
- Raft: port 9444

### Lattice on HSN

| Traffic | Direction | Size | Frequency |
|---------|-----------|------|-----------|
| Quorum Raft consensus | Quorum ↔ Quorum | State machine ops | On writes |
| Node-agent heartbeat + status | Agent → Quorum | Telemetry | 30s intervals |
| Allocation lifecycle | Quorum → Agent | Commands | Per allocation |
| Checkpoint coordination | Agent ↔ Quorum | Signals | On checkpoint |
| Capability reports | Agent → Quorum | ~2 KB | On change |

Quorum listens on HSN:
- gRPC: port 50051
- Raft: port 9000

### SPIRE bridges both networks

```
Node (management + HSN interfaces)
├── /run/spire/agent.sock  ← local unix socket, no network
│   ├── pact-agent obtains SVID → uses on management net (journal mTLS)
│   └── lattice-node-agent obtains SVID → uses on HSN (quorum mTLS)
│
├── Management NIC (1G)
│   └── pact-agent ←mTLS→ pact-journal:9443
│
└── HSN NIC (200G+, via cxi_rh)
    └── lattice-node-agent ←mTLS→ lattice-quorum:50051
```

X.509 certificates authenticate identity (SPIFFE ID or CN), not network interfaces.
The same SVID works on both networks. SPIRE agent is node-local — no network dependency
for identity acquisition.

### Boot ordering enforces this

```
T+0.0s  PXE boot via management net (OpenCHAMI)
T+0.1s  pact-agent starts as PID 1
T+0.2s  pact-agent gets SVID from SPIRE (local socket — no network)
T+0.3s  pact-agent connects to journal on management net (mTLS)
T+0.4s  pact pulls overlay, configures management interface (netlink)
T+0.5s  pact starts cxi_rh → HSN interface comes up
T+0.7s  pact starts lattice-node-agent (supervised service)
T+0.8s  lattice-node-agent gets SVID from SPIRE (local socket)
T+0.9s  lattice-node-agent connects to quorum on HSN (mTLS)
T+1.0s  Node fully operational on both networks
```

Management network MUST be available before HSN — it's the PXE boot network.
HSN comes up only after pact starts `cxi_rh` (a supervised service, Phase 5).
Therefore pact cannot use HSN for its own communication — it's not available
during early boot.

### Co-located mode

When journal and quorum share physical nodes:

```
Co-located node:
├── Management NIC (1G):
│   ├── pact-journal gRPC :9443
│   └── pact-journal Raft :9444
│
├── HSN NIC (200G+):
│   ├── lattice-quorum gRPC :50051
│   └── lattice-quorum Raft :9000
│
└── SPIRE agent socket (shared)
```

Each system listens on its own network. No port conflicts.
Both use SPIRE SVIDs — same trust domain, different network interfaces.

## Rationale

### Why management net for pact (not HSN)?

1. **Bootstrap ordering**: HSN is not available during early boot. Pact must
   connect to the journal to get the overlay that configures HSN.

2. **Failure isolation**: management net down → pact uses cached config (A9),
   lattice continues on HSN. HSN down → lattice pauses, pact continues managing
   nodes. Clean failure boundaries.

3. **Security boundary**: admin operations (shell, exec) should traverse the
   management network, not the workload network.

4. **Bandwidth is sufficient**: 10,000 nodes × 200 KB overlay = 2 GB.
   With 3-5 journal servers on 1G management NICs = 3-5 Gbps aggregate.
   Zstd-compressed overlays (~100 KB actual) = ~1 GB total = 2-3 seconds.
   Within the boot time target (A8: <2s with warm journal).

### Why HSN for lattice (not management)?

1. **Bandwidth**: telemetry from 10,000 nodes at 30s intervals, plus allocation
   lifecycle events, would saturate 1G management net.

2. **Latency**: Raft consensus and scheduler decisions need low latency.
   Slingshot provides sub-microsecond latency vs milliseconds on 1G Ethernet.

3. **Consistency**: workload traffic (MPI, NCCL, storage) already runs on HSN.
   Lattice managing workloads on the same network is natural.

### Failure isolation matrix

| Network down | Pact | Lattice | Workloads |
|-------------|------|---------|-----------|
| Management only | Journal unreachable. Agents use cached config (A9). Shell/exec unavailable. | Unaffected. | Running workloads continue. |
| HSN only | Unaffected. Admin access works. | Quorum unreachable. No new scheduling. | MPI/NCCL fails. Running jobs may checkpoint. |
| Both | BMC console only (F6). | Everything down. | Everything down. |
| Neither | Normal operation. | Normal operation. | Normal operation. |

## Trade-offs

- (+) Clean failure isolation — each system survives the other's network failure
- (+) No HSN dependency for pact — simpler boot sequence, fewer failure modes
- (+) Admin operations on management net — standard HPC security practice
- (+) SPIRE bridges both networks cleanly — same identity, different interfaces
- (+) Co-located mode works naturally — different ports on different NICs
- (-) Boot overlay streaming limited by 1G management net bandwidth
  (mitigated: zstd compression, 3-5 journal servers, overlays are small)
- (-) Two networks to monitor for full-system health
- (-) If management net is unreliable, pact operations are degraded even though
  HSN is fine (mitigated: cached config, A9)

## Consequences

- pact-journal configuration binds to management network interface
- lattice-quorum configuration binds to HSN interface
- pact-agent config specifies journal endpoints on management IP
- lattice-node-agent config specifies quorum endpoints on HSN IP
- SPIRE trust domain covers both networks (certs are interface-agnostic)
- Monitoring must cover both networks for complete system health visibility
- Network configuration in vCluster overlays must specify both interfaces
- For scale beyond ~50,000 nodes, boot overlay streaming may need to move to
  HSN or use a multicast/CDN approach on management net

## References

- ADR-001: Raft quorum deployment modes (standalone/co-located)
- ADR-006: Pact as init (boot ordering, service supervision)
- ADR-008: Node enrollment (management net for enrollment, HSN for post-boot)
- ADR-015: hpc-core shared contracts (network-agnostic identity)
- specs/invariants.md: R3 (quorum ports), A8 (boot time target), A9 (cached config)
- specs/failure-modes.md: F3 (partition), F28 (network config failure)
