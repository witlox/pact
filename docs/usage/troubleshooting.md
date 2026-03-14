# Troubleshooting

## Agent Cannot Connect to Journal

**Symptoms**: Agent logs show connection errors. `pact status` returns exit code 5 (timeout).

**Check 1: Network connectivity**
```bash
# From the agent node, verify the journal port is reachable
nc -zv journal-1.mgmt 9443
```

**Check 2: Journal is running**
```bash
# On the journal node
systemctl status pact-journal
journalctl -u pact-journal --since "5 min ago"
```

**Check 3: TLS configuration**
```bash
# Verify the CA certificate matches
openssl x509 -in /etc/pact/ca.crt -noout -subject -issuer

# Test TLS handshake
openssl s_client -connect journal-1.mgmt:9443 -CAfile /etc/pact/ca.crt
```

**Check 4: Agent config**

Verify `endpoints` in agent.toml points to the correct journal addresses:
```toml
[agent.journal]
endpoints = ["journal-1.mgmt:9443", "journal-2.mgmt:9443", "journal-3.mgmt:9443"]
tls_enabled = true
tls_cert = "/etc/pact/agent.crt"
tls_key = "/etc/pact/agent.key"
tls_ca = "/etc/pact/ca.crt"
```

**Check 5: Firewall**

Ensure port 9443 (gRPC) and 9444 (Raft) are open between journal nodes, and
port 9443 is open from compute nodes to journal nodes.

---

## Raft Leader Election Issues

**Symptoms**: Journal logs show repeated election timeouts. No leader elected.
CLI commands hang or return timeout errors.

**Check 1: Quorum availability**

A 3-node quorum needs at least 2 nodes. A 5-node quorum needs at least 3.
Verify all journal nodes are running:

```bash
for host in journal-1.mgmt journal-2.mgmt journal-3.mgmt; do
    echo "$host: $(nc -zv $host 9443 2>&1)"
done
```

**Check 2: Clock synchronization**

Raft is sensitive to clock skew. Verify NTP/chrony is running on all journal nodes:
```bash
chronyc tracking
```

**Check 3: Raft peer configuration**

All nodes must have identical `[journal.raft] members` configuration. A mismatch
causes election failures. Verify on each node:
```bash
grep -A5 "journal.raft" /etc/pact/journal.toml
```

**Check 4: Data directory permissions**

The journal data directory must be writable by the pact user:
```bash
ls -la /var/lib/pact/journal/
```

**Check 5: Network partitions**

Raft port 9444 must be reachable between all journal nodes. Unlike the gRPC port,
this is peer-to-peer between journal nodes only:
```bash
nc -zv journal-2.mgmt 9444
```

---

## Drift Detection False Positives

**Symptoms**: `pact diff` shows drift for files or paths that should not be
monitored (logs, temp files, runtime state).

**Fix: Add patterns to the blacklist**

The blacklist excludes paths from drift detection. Edit the agent config:

```toml
[agent.blacklist]
patterns = [
    "/tmp/**",
    "/var/log/**",
    "/proc/**",
    "/sys/**",
    "/dev/**",
    "/run/user/**",
    "/run/pact/**",
    "/run/lattice/**",
    # Add your exclusions here:
    "/var/cache/**",
    "/home/*/.bash_history"
]
```

After updating the config, restart the agent:
```bash
systemctl restart pact-agent
```

**Understanding the blacklist-first model**: pact monitors everything by default
and excludes via blacklist (see ADR-002). This is the opposite of most config
management tools which declare what to watch. The blacklist approach ensures
nothing is missed, but means you need to explicitly exclude noisy paths.

---

## Shell Command Blocked by Whitelist

**Symptoms**: `pact exec` or `pact shell` returns exit code 6 with
"command not whitelisted".

**Check 1: Current whitelist mode**

```bash
grep whitelist_mode /etc/pact/agent.toml
```

| Mode | Behavior |
|------|----------|
| `strict` | Only explicitly whitelisted commands allowed |
| `learning` | All commands allowed, non-whitelisted ones logged |
| `bypass` | All commands allowed (development only) |

**Fix for development**: Set `whitelist_mode = "learning"` or `"bypass"`.

**Fix for production**: Add the command to the whitelist. The whitelist is managed
via the vCluster overlay policy. Contact your platform admin to update it.

**Workaround**: If you need immediate unrestricted access, enter emergency mode:
```bash
pact emergency start -r "need to run diagnostics command XYZ"
# Run your command
pact exec node-042 -- your-command
pact emergency end
```

---

## Emergency Mode Stuck

**Symptoms**: A node is in emergency mode but the admin who started it is
unavailable. Other admins cannot make changes that conflict with the
emergency session.

**Fix: Force-end the emergency**

A `pact-platform-admin` can force-end another admin's emergency session:

```bash
pact emergency end --force
```

This records the force-end in the journal audit log, including who ended it
and the original emergency reason.

**If the CLI cannot reach the journal**: If the journal itself is the problem
(which is why emergency mode was started), you need to fix journal connectivity
first. Check the Raft leader election section above.

**Last resort**: BMC console access provides unrestricted bash on the node,
bypassing pact entirely. This is the out-of-band fallback when pact itself
is not functioning.

---

## Approval Workflow Issues

### Approval request expired

**Symptoms**: A commit on a regulated vCluster was submitted but nobody approved
it within the timeout (default 30 minutes). The change was rolled back.

**Fix**: Resubmit the change and coordinate with an approver in advance:

```bash
# Resubmit
pact commit -m "add audit-forwarder (re-submit after timeout)"

# Tell the approver to check immediately
# Approver runs:
pact approve list
pact approve accept ap-XXXX
```

**Adjust timeout**: If 30 minutes is too short for your workflow, update the
vCluster policy:
```toml
[vcluster.sensitive-compute.policy]
approval_timeout_seconds = 3600   # 1 hour
```

### Cannot approve own request

**Symptoms**: `pact approve accept` returns an authorization error when trying
to approve your own request.

This is by design. Two-person approval requires a different admin to approve.
The approver must have `pact-regulated-{vcluster}` or `pact-platform-admin` role.

### No approvers available

If no other admin with the required role is available, a `pact-platform-admin`
can approve any request. If no platform admin is available, the change must
wait or be submitted through the emergency mode workflow (which has its own
audit requirements).

---

## Agent Reports Wrong Capabilities

**Symptoms**: `pact cap` shows incorrect GPU count, memory, or network
capabilities.

**Check 1: Capability manifest**

The agent reads capabilities from a JSON manifest:
```bash
cat /run/pact/capability.json
```

**Check 2: GPU detection**

If GPU capabilities are wrong, check the GPU backend:
```bash
# For NVIDIA
nvidia-smi -L

# For AMD
rocm-smi --showproductname
```

**Check 3: Poll interval**

The agent polls GPU status periodically. Check the config:
```toml
[agent.capability]
gpu_poll_interval_seconds = 30
```

A recently failed GPU may not be reflected until the next poll.

---

## Journal Data Directory Full

**Symptoms**: Journal logs show write errors. Raft cannot commit new entries.

**Check disk usage**:
```bash
df -h /var/lib/pact/journal/
du -sh /var/lib/pact/journal/*
```

**Fix 1: Trigger a Raft snapshot**

Snapshots compact the log. The snapshot interval is configured in the journal:
```toml
[journal.raft]
snapshot_interval = 10000   # Entries between snapshots
```

Reduce this value and restart to trigger more frequent compaction.

**Fix 2: Expand storage**

If the data directory is genuinely too small for your workload, expand the
underlying volume.

---

## Common Error Messages

| Message | Cause | Fix |
|---------|-------|-----|
| `No auth token found` | Missing OIDC token | Set `PACT_TOKEN` or write to `~/.config/pact/token` |
| `No vCluster specified` | Missing vCluster scope | Use `--vcluster` or set `PACT_VCLUSTER` |
| `connection refused` | Journal not running or wrong endpoint | Check journal status and endpoint config |
| `certificate verify failed` | TLS cert mismatch | Verify CA cert is correct on both sides |
| `policy: denied` | OPA rejected the operation | Check your role has the required permissions |
| `approval required` | Regulated vCluster | Another admin must approve (see workflow above) |
| `commit window expired` | Time window for changes has closed | Run `pact extend` or `pact commit` first |
