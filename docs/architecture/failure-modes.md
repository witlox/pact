# Failure Modes and Recovery

## Journal Quorum Failures

### Single Node Failure
- **Impact**: Quorum maintained (2/3 or 3/5 nodes)
- **Detection**: Raft heartbeat timeout (1.5-3s)
- **Recovery**: Automatic leader re-election, failed node rejoins on restart
- **Data**: No data loss — committed entries are replicated

### Quorum Loss (Majority Down)
- **Impact**: No writes accepted, reads still served from local state
- **Detection**: Raft cannot elect leader
- **Recovery**: Restore majority of nodes, cluster auto-recovers
- **Agent behavior**: Runs in disconnected mode, buffers events

### Split Brain
- **Impact**: Impossible by Raft design (majority required for writes)
- **Detection**: Minority partition detects it's not leader
- **Recovery**: Automatic on network heal

## Agent Failures

### Agent Cannot Connect to Journal
- **Impact**: No config updates, no audit logging
- **Detection**: Connection timeout, subscription backoff
- **Recovery**: Exponential backoff reconnect (1s base, 60s max, 100 attempts)
- **Behavior**: Agent continues with cached config in observe-only mode

### Agent Crash (pact as init)
- **Impact**: All supervised services orphaned
- **Detection**: systemd/PID 1 watchdog (if configured)
- **Recovery**: Agent restart re-reads state, re-supervises services
- **Data**: Capability report regenerated on boot

### Drift Detection False Positive
- **Impact**: Unnecessary commit window opened
- **Detection**: Admin reviews drift via `pact diff`
- **Recovery**: Add path to blacklist patterns, drift resets on commit

## Network Failures

### Agent-Journal Partition
- **Impact**: Config subscription disconnected
- **Detection**: gRPC stream error
- **Recovery**: Reconnect with `from_sequence` (at-least-once delivery)
- **Conflict resolution**: Journal-wins after grace period (ConflictManager)

### Inter-Journal Partition
- **Impact**: Raft replication paused for minority side
- **Detection**: Raft log divergence
- **Recovery**: Automatic reconciliation on heal, minority replays missed entries

## Emergency Mode Failures

### Emergency Mode Stuck
- **Impact**: Extended commit window, reduced automation
- **Detection**: Stale emergency detection (expiry without resolution)
- **Recovery**: Platform admin force-end (`pact emergency end --force`)
- **Audit**: All emergency actions logged regardless of mode

### Emergency Mode Unauthorized Entry
- **Impact**: Blocked by RBAC (P8: AI agents cannot enter)
- **Detection**: PolicyService evaluation returns Deny
- **Recovery**: Human admin must initiate emergency mode
