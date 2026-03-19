# Agentic API (MCP Tool-Use)

MCP server wrapping pact gRPC API for Claude Code-style AI agent integration.
Authenticates as pact-service-ai principal with scoped permissions.

## Tools

- pact_status: node/vCluster state query
- pact_diff: drift details
- pact_commit: commit pending changes
- pact_apply: apply config spec
- pact_rollback: revert to previous state
- pact_log: query history
- pact_exec: run diagnostic command on node
- pact_cap: node hardware capability report
- pact_query_fleet: fleet-wide health query
- pact_emergency: start/end emergency (typically restricted to human admins)
- pact_service_status: query service health across nodes

## Security

- Service principal with limited write permissions
- Read operations broadly permitted
- Write operations require explicit policy authorization
- Emergency mode typically restricted to human admin principals
- All operations logged as author: service/ai-agent/<name>

## Example: AI Agent Investigating GPU Failures

```
1. pact_query_fleet(vcluster="ml-training", capability_filter="gpu_health=degraded")
   → 3 nodes with degraded GPUs

2. pact_exec(node="node042", command="nvidia-smi -q -d ECC")
   → ECC error details

3. pact_log(scope="node042", entry_types=["capability_change"])
   → degradation history

4. pact_apply(scope="ml-training", config={...}, message="auto-remediation")
   → applied to all nodes, policy authorized
```

## Supercharged Command Tools

Read-only cross-system views exposed as MCP tools, delegating to the lattice
scheduler via `DelegationConfig`:

| MCP Tool | CLI Equivalent | Description |
|----------|---------------|-------------|
| `pact_jobs_list` | `pact jobs list` | List running allocations with node/vCluster filters |
| `pact_queue_status` | `pact queue` | Queue depth and scheduling status per vCluster |
| `pact_cluster_health` | `pact cluster` | Combined pact journal + lattice Raft health |
| `pact_system_health` | `pact health` | Combined health check across pact and lattice |
| `pact_accounting` | `pact accounting` | Resource usage (CPU/GPU hours, storage) per tenant |

These tools require `PACT_LATTICE_ENDPOINT` (and optionally `PACT_LATTICE_TOKEN`)
to be set. Without a lattice connection, they return descriptive error messages.

Write commands (`pact jobs cancel`) remain human-only unless explicitly
authorized via policy. `pact audit` is useful but may expose sensitive data and
should be scoped carefully.
