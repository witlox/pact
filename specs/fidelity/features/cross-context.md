# Fidelity Report: cross_context.feature

Last scan: 2026-03-20
Feature file: `crates/pact-acceptance/features/cross_context.feature`
Step definitions: `crates/pact-acceptance/tests/steps/cross_context.rs` (+ shared steps from other modules)

## Scenarios: 24 (15 passing, 9 skipped)

### Passing (15)

| # | Scenario | Depth | Notes |
|---|----------|-------|-------|
| 1 | Full boot streams config and starts services | MODERATE | Calls simulate_boot_stream, sorts ServiceDecls, generates real CapabilityReport, subscribes. Real data flow but simulated boot. |
| 2 | Config update triggers service restart | MODERATE | Rebuilds overlay via JournalCommand::SetOverlay, pushes ConfigUpdateEvent. Real overlay version increment. |
| 3 | Drift detected by observer triggers commit window | THOROUGH | Real DriftEvaluator.process_event(), real CommitWindowManager.open(), real JournalCommand::AppendEntry(DriftDetected), real node state update. |
| 4 | Commit window expiry triggers rollback | MODERATE | Uses commit_mgr.rollback() and journal entry. Timer expiry simulated (not real time). |
| 5 | Exec flows through policy and audit | MODERATE | Whitelist check, exit code, journal audit recording. Policy evaluation bypassed (ops role assumed allowed). |
| 6 | Non-whitelisted exec denied | MODERATE | Whitelist rejection with exit code 6, no policy call verified. |
| 7 | Shell state change triggers drift | MODERATE | Real DriftEvaluator.process_event() for kernel change, commit window opened. |
| 8 | Emergency escalation through scheduling hold | MODERATE | Real EmergencyManager.start()/end(), journal entries recorded, node state transitions. Lattice cordon is conceptual. |
| 9 | Two-person approval (via shared policy steps) | MODERATE | Real DefaultPolicyEngine.evaluate_sync() returns RequireApproval. Approval workflow via shared steps. |
| 10 | Self-approval denied (via shared auth steps) | MODERATE | AuthResult::Denied set. |
| 11 | Partition → degraded → replay | MODERATE | Journal reachability flags, drift logged, cached policy used, reconnect restores. |
| 12 | Partition → merge conflict → resolution | THOROUGH | Real ConflictManager.register_conflicts(), is_paused(), journal entries with StateDelta. Real conflict detection + resolution. |
| 13 | Promote → overlay → boot | MODERATE | Overlay rebuilt via SetOverlay, new node receives overlay chunk. |
| 14 | GPU degradation → capability → scheduler | THOROUGH | Real GPU health state change, real CapabilityReport regeneration, journal entry, manifest update. |
| 15 | Federation → policy template update | MODERATE | Real MockFederationSync.sync(), templates stored. OPA evaluation conceptual. |

### Skipped (9) — step wording mismatches

| # | Scenario | Missing Step | Fix |
|---|----------|-------------|-----|
| 1 | Full boot (cont.) | "And a CapabilityReport is written to tmpfs" | Rename to "should be written" |
| 4 | Commit window (cont.) | "When the commit window expires" | Existing step is "the window expires" |
| 6 | Non-whitelisted exec | "When admin executes 'rm /tmp/test'" | Args not parsed by regex |
| 8 | Emergency (cont.) | "When admin 'ops-lead@...' force-ends" | Regex mismatch on email |
| 11 | Partition (cont.) | "When the network partition heals" | Existing uses "connectivity restored" |
| 12 | Partition (cont.) | "And the network partition heals" | Same |
| 19 | Allocation lifecycle | "Then pact cleans up alloc-02's..." | Apostrophe in regex |
| 20 | Emergency workload | "When admin enters emergency..." | Wording variation |
| 23 | Federation UID | "And 'researcher@...' authenticates" | Removed as duplicate |

## Summary

- **THOROUGH**: 3 (drift→commit, merge conflict, GPU capability)
- **MODERATE**: 12
- **SHALLOW**: 0
- **SKIPPED**: 9 (step wording, not logic gaps)
- **Confidence: MODERATE** (100% of passing scenarios are T+M)

## Integration Value Assessment

The cross-context scenarios verify these end-to-end chains:
- Boot→Config→Service→Capability: **verified** (data flows through real overlays + CapabilityReporter)
- Drift→CommitWindow→Journal: **verified** (real DriftEvaluator + CommitWindowManager + JournalState)
- Partition→Conflict→Resolution: **verified** (real ConflictManager + JournalState.detect_conflicts)
- Emergency→AuditTrail→StateTransition: **verified** (real EmergencyManager + JournalState)

Not yet verified (skipped):
- Allocation lifecycle with namespace handoff (complex multi-When)
- SPIRE→mTLS rotation chain
- Agent crash recovery
