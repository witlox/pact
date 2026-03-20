# Workflow Orchestration

## Profile Overview

| Phase | Profile | Input | Output | Entry Criteria | Exit Criteria |
| --- | --- | --- | --- | --- | --- |
| 1 | `analyst.md` | Domain knowledge (user's head) | `/specs/` complete | Project start | Graduation checklist passed |
| 2 | `architect.md` | `/specs/` | `/specs/architecture/` | Analyst graduation | Architecture consistency checks passed |
| 3 | `adversary.md` (arch mode) | `/specs/` + `/specs/architecture/` | Findings report | Architecture complete | All critical/high findings resolved |
| 4 | `implementer.md` (per feature) | `/specs/` + `/specs/architecture/` | `/src/` (feature) | Adversary sign-off on architecture | Feature tests pass |
| 4b | `auditor.md` (per feature) | Feature code + specs + tests | `specs/fidelity/` update | Feature tests pass | Fidelity assessed |
| 4c | `implementer.md` (test hardening) | Fidelity report | Deepened tests | Confidence < HIGH | Confidence >= HIGH |
| 5 | `adversary.md` (impl mode) | Everything + fidelity index | Findings report | Feature confidence HIGH | All critical/high findings resolved |
| 6 | `auditor.md` (full scan) | Full codebase | `specs/fidelity/` refresh | Pre-integration | Index current, no regressions |
| 7 | `integrator.md` | Everything + fidelity index | Integration report + tests | Full audit clean | Graduation criteria met |

## Key change: Definition of Done

A feature is **done** when ALL of the following are true:

1. All Gherkin scenarios pass (`cargo test`)
2. Fidelity confidence is **HIGH** for the feature (>80% scenarios THOROUGH+)
3. No DIVERGENT mocks on critical paths for this feature
4. Relevant ADRs are ENFORCED (not just DOCUMENTED)
5. Adversary sign-off with fidelity index as input

A feature with passing tests but LOW confidence is **not done** — it is
**undertested**. The implementer must deepen the tests before adversary review.

## Iteration Loops

### Loop A: Architecture Refinement (Phases 2-3)

```
architect → adversary → [findings] → architect → adversary → ... until clean
```

### Loop B: Feature Implementation (Phases 4-4c)

```
implementer(feature_N) → auditor(feature_N) → [confidence < HIGH?]
  → implementer(harden tests) → auditor(feature_N) → ... until HIGH
```

This loop is NEW. The implementer no longer self-certifies "done."
The auditor provides an independent measurement. Only when confidence
reaches HIGH does the feature proceed to adversary review.

### Loop C: Adversary Review (Phase 5)

```
adversary(feature_N, with fidelity index) → [findings] →
  implementer(feature_N) → auditor(feature_N) → adversary → ... until clean
```

Note: the adversary now receives the fidelity index as input. This means
the adversary can focus on design and logic flaws rather than spending time
checking whether tests are thorough — the auditor already measured that.

If the adversary finds gaps the auditor missed (e.g., a scenario that's
THOROUGH on the wrong thing), the adversary can flag it and the auditor
profile should be updated to catch similar patterns.

### Loop D: Integration (Phases 6-7)

```
auditor(full scan) → [regressions?]
  → implementer(affected features) → auditor(affected) → ... until clean
integrator → [findings]
  → implementer(affected features) → adversary → integrator → ... until clean
```

The full audit scan before integration catches regressions: features that were
HIGH confidence but degraded due to cross-feature changes, new mocks that
weren't assessed, ADRs that lost enforcement.

### Escalation Paths

Any phase can escalate to a prior phase:

* Implementer → Architect (interface doesn't work)
* Implementer → Analyst (spec is ambiguous or incomplete)
* Adversary → Architect (structural flaw)
* Adversary → Analyst (spec gap)
* Integrator → Architect (cross-cutting structural issue)
* **Auditor → Implementer** (tests are shallow, need deepening)
* **Auditor → Architect** (mock doesn't match real trait contract — interface problem)
* **Auditor → Analyst** (spec is ambiguous — can't determine correct assertion)
* **Adversary → Auditor** (adversary suspects test is THOROUGH on wrong behavior)

Escalations go to `/specs/escalations/` and must be resolved before the
escalating phase can complete.

## Usage with Claude Code

### Swapping Profiles

Option 1 — Use the provided script:

```
./switch-profile.sh analyst
./switch-profile.sh architect
./switch-profile.sh adversary
./switch-profile.sh implementer
./switch-profile.sh integrator
./switch-profile.sh auditor
```

Option 2 — Manual copy:

```
cp .claude/analyst.md .claude/CLAUDE.md
```

Option 3 — Parameterized launch (if using --prompt):

```
claude --prompt "You are operating under the auditor profile. Read .claude/auditor.md for your instructions."
```

**Note:** Profiles are written to `.claude/CLAUDE.md` (gitignored), not the root `CLAUDE.md`.
Claude loads both files, so the profile automatically gets the project context from the root.

### Feature Scoping

The implementer and auditor profiles both accept feature scope:

1. Editing the first line of CLAUDE.md after copying:

   ```
   # CURRENT SCOPE: Feature [name] — [specs/features/feature-name.feature]
   ```

2. Or stating it in your first message to Claude Code:

   ```
   "Audit feature [name]. Specs are in specs/features/[name].feature."
   ```

### Session Start Protocol

At the start of any session (regardless of profile), Claude should:

1. Read `specs/fidelity/INDEX.md` if it exists
2. Note the last scan date and any features with LOW/NONE confidence
3. If working on a specific feature, read its fidelity detail file
4. Adjust behavior accordingly:
   - **Implementer**: don't mark done if confidence < HIGH
   - **Adversary**: prioritize reviewing features with LOW fidelity
   - **Integrator**: flag integration of LOW-confidence features as risky

This is triggered by the root CLAUDE.md instruction (see Integration Guide).

## Directory Structure (Full)

```
project-root/
├── CLAUDE.md                          # Project context (stable, always loaded)
├── switch-profile.sh                  # Profile switcher script
├── .claude/
│   ├── CLAUDE.md                      # Active profile (swapped per phase, gitignored)
│   ├── WORKFLOW.md                    # This file
│   ├── analyst.md
│   ├── architect.md
│   ├── adversary.md
│   ├── implementer.md
│   ├── integrator.md
│   └── auditor.md                     # NEW
├── specs/
│   ├── domain-model.md
│   ├── ubiquitous-language.md
│   ├── invariants.md
│   ├── assumptions.md
│   ├── features/
│   │   ├── feature-a.feature
│   │   ├── feature-b.feature
│   │   └── ...
│   ├── cross-context/
│   │   ├── interactions.md
│   │   └── cross-context.feature
│   ├── failure-modes.md
│   ├── architecture/
│   │   ├── module-map.md
│   │   ├── dependency-graph.md
│   │   ├── interfaces/
│   │   ├── data-models/
│   │   ├── events/
│   │   ├── error-taxonomy.md
│   │   └── enforcement-map.md
│   ├── fidelity/                      # NEW — auditor output
│   │   ├── INDEX.md                   # Summary dashboard
│   │   ├── features/                  # Per-feature fidelity reports
│   │   ├── mocks/                     # Per-trait mock fidelity
│   │   ├── adrs/                      # ADR enforcement status
│   │   └── gaps.md                    # Cross-cutting gaps
│   ├── integration/
│   │   └── [generated by integrator]
│   └── escalations/
│       └── [filed by any phase]
└── src/
    └── [generated by implementer]
```

## Checkpoints and Human Gates

While the workflow can run LLM-to-LLM through the phases, certain transitions
benefit from human review:

**Recommended human gates:**

* After Phase 1 (analyst): Review the specs. Last cheap opportunity to catch
  fundamental misunderstandings.
* After Phase 3 (adversary on architecture): Review findings. Architectural
  changes after implementation starts are expensive.
* **After Phase 4b (first audit)**: Review the fidelity report. This tells you
  whether Claude's tests are actually thorough or just passing. Catching
  shallow tests here is far cheaper than discovering it during integration.
* After Phase 7 (integrator): Review integration report before declaring
  the system complete.

**Optional human gates:**

* After each Phase 4 iteration: Review escalations, if any.
* **After Phase 6 (full audit)**: Review the fidelity delta. If confidence
  regressed on features that were previously HIGH, investigate why.

## When Things Go Wrong

**Symptom: Architect can't produce clean interfaces.**
Cause: Usually a spec gap. The domain model has ambiguity the architect can't resolve.
Action: Go back to analyst. Load the analyst profile with the architect's specific questions.

**Symptom: Implementer keeps escalating interface changes.**
Cause: Either the architecture doesn't fit reality, or the implementer is fighting the design.
Action: If escalations are concentrated in one area, that area needs architectural rework.
If they're scattered, the implementer may be drifting from the architecture.

**Symptom: Auditor reports LOW confidence on a feature the implementer marked as done.**
Cause: Tests are passing but shallow. Common patterns:
  - Step definitions assert on mocks, not real behavior
  - `Then` steps check status codes but not state changes
  - `Given` steps bypass validation by injecting state directly
  - No negative/edge case scenarios
Action: Implementer deepens tests. Specifically: read the auditor's per-feature
fidelity report, which lists exactly which scenarios are shallow and what they're
missing. Fix those specific gaps, don't add tests blindly.

**Symptom: Auditor reports DIVERGENT mocks.**
Cause: Mock implementation doesn't match the real trait contract. The mock
always succeeds, never returns errors, or accepts inputs the real impl rejects.
Action: Either fix the mock to be faithful, or add integration tests that
exercise the real implementation. The auditor escalates to the architect if
the trait interface itself is the problem (real impl has constraints not
expressed in the trait signature).

**Symptom: Integrator finds many issues despite HIGH fidelity scores.**
Cause: Features were tested in isolation with faithful mocks, but the mocks
don't capture cross-feature interactions. The fidelity index measures
per-feature depth, not cross-feature integration.
Action: Add cross-context scenarios (`specs/cross-context/`). The integrator
should produce these; the auditor should then assess their depth too.

**Symptom: Everything passes, fidelity is HIGH, but the system doesn't feel right.**
Cause: Likely semantic drift — the code satisfies the tests but doesn't match
the domain intent. The specs themselves may be wrong.
Action: Go back to analyst. Walk through the system behavior with the user,
comparing what the system does to what the domain expects. Update specs,
cascade changes through architect → adversary → implementer → auditor.

**Symptom: Full audit scan takes too long / exceeds context window.**
Cause: Large codebase with many features.
Action: Run audits incrementally per feature. Use the full scan only before
integration phases. The INDEX.md tracks per-feature last-scan dates so you
can prioritize stale entries.
