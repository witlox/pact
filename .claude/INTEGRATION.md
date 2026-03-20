# Integration Guide: Auditor Profile + Fidelity Index

## What this adds to your project

Three things:

1. **`.claude/auditor.md`** — A new profile in your existing workflow system
2. **`specs/fidelity/`** — A directory for the fidelity index and detail files
3. **Updates to `CLAUDE.md`** and `switch-profile.sh` to wire it in

## Step 1: Add files

```bash
# Copy the auditor profile
cp auditor.md .claude/auditor.md

# Create the fidelity directory structure
mkdir -p specs/fidelity/features specs/fidelity/mocks specs/fidelity/adrs
cp INDEX.md specs/fidelity/INDEX.md

# Create placeholder for cross-cutting gaps
cat > specs/fidelity/gaps.md << 'EOF'
# Cross-Cutting Gaps
Last scan: never

## Dead Specs
_Feature files with no step definitions._

## Orphan Tests  
_Test files that don't map to any feature spec._

## Stale Specs
_Specs whose language doesn't match current code._

## Uncovered Modules
_Source modules with no test files._

## Feature Flag Gaps
_Code behind feature flags with no gated test scenarios._

| Flag | Modules | Has Gated Tests | Gap |
|------|---------|-----------------|-----|
| ebpf | observer/ | ? | ? |
| nvidia | capability/ | ? | ? |
| amd | capability/ | ? | ? |
| systemd | supervisor/ | ? | ? |
| opa | rules/ | ? | ? |
| federation | federation/ | ? | ? |
| spire | identity/ | ? | ? |
EOF
```

## Step 2: Update switch-profile.sh

Add "auditor" to the VALID_PROFILES array:

```bash
# Change this line:
VALID_PROFILES=("analyst" "architect" "adversary" "implementer" "integrator")

# To this:
VALID_PROFILES=("analyst" "architect" "adversary" "implementer" "integrator" "auditor")
```

## Step 3: Add to root CLAUDE.md

Add this section to the root `CLAUDE.md` (which is always loaded alongside any profile).
Place it after the "Design decisions" section:

```markdown
## Test fidelity index

The project maintains a fidelity index at `specs/fidelity/INDEX.md` that tracks what
the test suite ACTUALLY verifies versus what the specs claim. This is distinct from
code coverage — it measures assertion depth and mock faithfulness.

**Read `specs/fidelity/INDEX.md` when:**
- Starting work on any feature (check its confidence level first)
- The implementer marks a feature as "done" (verify confidence is HIGH)
- Before any release or integration phase

**The fidelity index is maintained by the auditor profile:**
```
./switch-profile.sh auditor
```

**Trigger commands (in auditor mode):**
- "audit" or "scan fidelity" — full scan, updates everything
- "audit [feature-name]" — scan one feature only
- "audit mocks" — scan mock fidelity only
- "audit adrs" — scan ADR enforcement only

**Other profiles reference the index but do not modify it:**
- **Implementer**: check fidelity before marking done. If confidence < HIGH,
  improve tests before closing the feature.
- **Adversary**: cross-reference fidelity index when reviewing. Shallow tests
  on critical paths should be flagged as findings.
- **Integrator**: the fidelity index is an input to integration assessment.
  Don't declare integration complete if critical features have LOW confidence.
```

## Step 4: Update WORKFLOW.md

Add the auditor to the workflow table and iteration loops.

### Workflow table addition:

```markdown
| Phase | Profile | Input | Output | Entry Criteria | Exit Criteria |
| --- | --- | --- | --- | --- | --- |
| * | `auditor.md` | Full codebase + specs + tests | `specs/fidelity/` | Any time | Index updated |
```

The auditor is not a sequential phase — it's callable at any point. Recommended
trigger points:

- **After Phase 4** (implementer): audit the feature just implemented
- **Before Phase 5** (adversary on impl): adversary reads fidelity index first
- **Before Phase 6** (integrator): full scan before integration assessment
- **On session start**: if the fidelity index is stale (>1 week or >20 commits
  since last scan), run a refresh

### New iteration loop:

```markdown
### Loop D: Fidelity Maintenance

```
implementer(feature_N) → auditor(feature_N) → [fidelity < HIGH?] → implementer(fix tests) → auditor(feature_N) → ... until HIGH
```
```

### Escalation path addition:

```markdown
* Auditor → Implementer (tests are shallow, need deepening)
* Auditor → Architect (mock doesn't match real contract — interface issue)
* Auditor → Analyst (spec is ambiguous — can't determine what "correct" means)
```

## How this works in practice

### Scenario: You finished implementing pact shell

```bash
# 1. Switch to auditor
./switch-profile.sh auditor

# 2. In Claude Code:
"audit pact-shell"

# Auditor reads:
#   - specs/features/pact-shell.feature
#   - finds step definitions in tests/e2e/
#   - traces assertions
#   - checks mock fidelity for ShellServer trait
#   - writes specs/fidelity/features/pact-shell.md
#   - updates INDEX.md

# Output might be:
#   Feature: pact-shell
#   Scenarios: 12, Confidence: LOW
#   - 8 scenarios only check command exit codes (SHALLOW)
#   - Shell output content never verified
#   - MockAuthProvider always returns authorized (DIVERGENT)
#   - No test for OIDC scope enforcement
#   - No test for command whitelist violation
#   
#   Priority: deepen auth and whitelist scenarios before marking done

# 3. Switch to implementer to fix
./switch-profile.sh implementer "pact-shell"

# 4. Fix tests, then re-audit
./switch-profile.sh auditor
"audit pact-shell"
# Now: Confidence: MODERATE → repeat until HIGH
```

### Scenario: Starting a new session on a different machine

```bash
git pull  # fidelity index syncs via git

./switch-profile.sh implementer "drift-detection"

# In Claude Code, the root CLAUDE.md tells Claude to check fidelity first.
# Claude reads specs/fidelity/INDEX.md, sees:
#   drift-detection: Confidence LOW, 3 SHALLOW scenarios
# Claude knows the tests can't be trusted as a "done" signal.
# It can also read specs/fidelity/features/drift-detection.md
# to see exactly which scenarios need deepening.
```

### Scenario: Full scan before a release

```bash
./switch-profile.sh auditor
"scan fidelity"

# Auditor runs all 4 phases across the entire project.
# Takes a while (multiple context windows for large codebases).
# Updates everything in specs/fidelity/.
# Produces a delta: "Since last scan: 3 features improved, 1 degraded,
#   2 new mocks assessed, ADR-016 now enforced."
```

## What the fidelity index does NOT do

- It does not run tests. It reads test code and evaluates what the assertions prove.
- It does not measure code coverage. Use `cargo tarpaulin` or `llvm-cov` for that.
- It does not fix anything. It reports. Other profiles act.
- It does not replace the adversary. The adversary finds design flaws and attack
  surfaces. The auditor measures whether existing tests actually verify what they
  claim to verify. Complementary roles.

## Cross-machine portability

The entire `specs/fidelity/` directory is plain markdown, committed to git.
No local state, no database, no embeddings. `git pull` and you have the full
fidelity picture on any machine.

The fidelity index IS the "compressed memory" for test quality — it's a
structured index with pointers to specific files and lines, readable by both
humans and Claude, portable via git.
