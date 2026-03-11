# Federation Model (via Sovra)

## Principle

Configuration state is site-local. Policy templates are federated.

## What federates

- Policy templates (regulated workload requirements) via Sovra mTLS
- Compliance reports (drift/audit summaries) via Sovra attestation
- Policy attestation (cryptographic proof of policy conformance)

## What stays site-local

- Configuration state (mounts, services, sysctl values)
- Drift events, admin sessions, commit history
- Capability reports (only meaningful to local scheduler)
- Shell/exec session logs

## Consistent with lattice

Lattice scheduler is site-local. Federation enables cross-site job submission.
pact follows the same boundary: config is local, policy is federated.
