# Security Architecture

## Authentication

### OIDC / JWT
- All API calls authenticated via Bearer JWT tokens in gRPC metadata
- Development: HS256 with shared secret
- Production: RS256 with JWKS endpoint (auto-refreshed, 1hr cache)
- Token claims: `sub` (principal), `pact_role` (authorization role)

### Machine Identity (mTLS)
- Agent-to-journal: mutual TLS with X.509 certificates
- Certificate fields: `tls_cert`, `tls_key`, `tls_ca` in agent config
- Journal validates client certificate against CA bundle
- Agent validates journal certificate against CA bundle

## Authorization (RBAC)

### Role Model
| Role | Scope | Permissions |
|------|-------|-------------|
| `pact-platform-admin` | Global | Full access, whitelist bypass (S2) |
| `pact-ops-{vcluster}` | Per-vCluster | Commit, rollback, exec, shell, service mgmt |
| `pact-viewer-{vcluster}` | Per-vCluster | Read-only: status, log, diff, read-only exec |
| `pact-regulated-{vcluster}` | Per-vCluster | Like ops, but requires two-person approval (P4) |
| `pact-service-agent` | Machine | Agent mTLS identity |
| `pact-service-ai` | Machine | MCP tools, no emergency mode (P8) |

### Policy Invariants
- **P1**: Identity required on all requests
- **P2**: Viewers read-only
- **P3**: Role scoped to correct vCluster
- **P4**: Regulated roles require two-person approval
- **P6**: Platform admin always authorized
- **P8**: AI agents cannot enter/exit emergency mode

### OPA Integration (ADR-003)
- Rego policies co-located on journal nodes
- Evaluated via localhost REST (`http://localhost:8181/v1/data/pact/authz`)
- Federation: policy templates synced via Sovra

## Shell Security (ADR-007)

### No SSH
- pact shell replaces SSH for all admin access
- BMC/Redfish console is the only out-of-band fallback

### Whitelist Enforcement
- Commands restricted via PATH symlinks (not command parsing)
- Session-specific directory: `/run/pact/shell/{session_id}/bin/`
- 37 default whitelisted commands (ps, top, nvidia-smi, etc.)
- State-changing commands classified (systemctl, modprobe → true)
- Learning mode captures denied commands for review

### PTY Isolation
- Restricted bash (rbash) prevents PATH modification
- `BASH_ENV=""`, `ENV=""` prevent startup file injection
- `HOME=/tmp` prevents home directory access
- `PROMPT_COMMAND` logs every command for audit

## Audit Trail
- Every operation logged as a ConfigEntry in the journal
- Immutable Raft-committed log
- EntryTypes: ExecLog, ShellSession, ServiceLifecycle, EmergencyStart/End
- Regulated vClusters: 7-year retention (audit_retention_days=2555)
