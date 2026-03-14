# Policy Engine Design (ADR-003)

## Architecture

Policy evaluation is co-located on journal nodes. The `PolicyService` gRPC
service handles all policy operations:

```
CLI/Agent → PolicyService (journal) → RbacEngine (pact-policy) → Decision
                                   → OPA sidecar (optional, for Rego policies)
```

## gRPC API

```protobuf
service PolicyService {
  rpc Evaluate(PolicyEvalRequest) returns (PolicyEvalResponse);
  rpc GetEffectivePolicy(GetPolicyRequest) returns (VClusterPolicy);
  rpc UpdatePolicy(UpdatePolicyRequest) returns (UpdatePolicyResponse);
  rpc ListPendingApprovals(ListApprovalsRequest) returns (ListApprovalsResponse);
  rpc DecideApproval(DecideApprovalRequest) returns (DecideApprovalResponse);
}
```

## RBAC Decisions

| Decision | Meaning | Action |
|----------|---------|--------|
| Allow | Authorized | Proceed |
| Deny { reason } | Not authorized | Return error with reason |
| Defer | Requires approval | Create PendingApproval, return approval_id |

## Two-Person Approval (P4)

For regulated vClusters (`two_person_approval = true`):

1. Regulated role submits state-changing action
2. PolicyService returns `Defer` with `pending_approval_id`
3. Approval persisted through Raft (`CreateApproval` command)
4. Second admin approves or rejects (`DecideApproval` command)
5. Self-approval denied (requester != approver)
6. Approvals expire after 24 hours

## VCluster Policy

```toml
[vcluster.ml-training]
drift_sensitivity = 2.0
base_commit_window_seconds = 900
emergency_window_seconds = 14400
regulated = false
two_person_approval = false
enforcement_mode = "observe"  # or "enforce"
supervisor_backend = "pact"    # or "systemd"
exec_whitelist = ["nvidia-smi", "dmesg"]
shell_whitelist = ["ls", "cat"]
emergency_allowed = true
audit_retention_days = 2555    # ~7 years for regulated
```

## OPA Integration

- OPA runs as a sidecar on journal nodes (port 8181)
- Rego policies pushed via OPA REST API
- Federation: policy templates synced via Sovra
- Fallback: built-in RbacEngine if OPA unavailable
