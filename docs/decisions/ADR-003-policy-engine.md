# ADR-003: Policy Engine Choice — OPA/Rego vs Cedar

## Status: Pending

## Leaning: OPA/Rego

Primary reason: Sovra compatibility. Sovra uses OPA. Federated policy templates need
a shared language. OPA as sidecar/REST from Rust is acceptable overhead (policy eval
is not on the hot path).

## Decision: End of Phase 1.
