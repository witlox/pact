# Federation Model (via Sovra)

## Principle

Configuration state is site-local. Policy templates are federated.

## Policy Language

OPA/Rego (see [ADR-003](../decisions/ADR-003-policy-engine.md)). Sovra uses OPA
natively, so federated policy templates are authored in Rego and distributed without
translation. This is the primary reason OPA was chosen over Cedar.

## What federates

- Rego policy templates (regulated workload requirements) via Sovra mTLS
- Compliance reports (drift/audit summaries) via Sovra attestation
- Policy attestation (cryptographic proof of policy conformance)

## What stays site-local

- Configuration state (mounts, services, sysctl values)
- Drift events, admin sessions, commit history
- Capability reports (only meaningful to local scheduler)
- Shell/exec session logs
- OPA data (role mappings, vCluster-specific policy overrides)

## Federation Sync

pact-policy syncs Rego templates from Sovra on a configurable interval (default 300s).
Templates are stored locally and loaded into OPA as bundles. Site-local data (role
mappings, vCluster config) is pushed to OPA separately and never leaves the site.

## Consistent with lattice

Lattice scheduler is site-local. Federation enables cross-site job submission.
pact follows the same boundary: config is local, policy is federated.
