# Fidelity Index

Last scan: never
Scanned by: awaiting first audit

## How to read this file

This file is the entry point for understanding what this project ACTUALLY verifies
versus what its specs CLAIM is verified. It is maintained by the auditor profile
(`./switch-profile.sh auditor`).

**Confidence levels:**
- **HIGH**: >80% of scenarios are THOROUGH or INTEGRATION depth
- **MODERATE**: >50% THOROUGH+, no critical gaps
- **LOW**: <50% THOROUGH+, or critical paths undertested
- **NONE**: no tests, or tests exist but assert nothing meaningful

**Assertion depth:**
- **INTEGRATION**: runs against real services (feature-gated)
- **THOROUGH**: asserts actual state through real or faithfully-mocked code
- **MODERATE**: asserts real return values but via mocked dependencies
- **SHALLOW**: asserts status codes, booleans, or mock invocation only
- **STUB**: step def exists but is empty / unimplemented
- **NONE**: no test exists for this criterion

## Summary

| Metric | Count |
|--------|-------|
| Feature files scanned | — |
| Total scenarios | — |
| THOROUGH+ scenarios | — |
| SHALLOW or worse | — |
| Mock traits assessed | — |
| FAITHFUL mocks | — |
| ADRs total | — |
| ADRs ENFORCED | — |

## Feature Fidelity

| Feature | Scenarios | Thorough | Moderate | Shallow | Stub/None | Confidence |
|---------|-----------|----------|----------|---------|-----------|------------|
| _awaiting first scan_ | | | | | | |

Detail files: `specs/fidelity/features/<feature-name>.md`

## Mock Fidelity

| Trait | Real Impls | Mock Rating | Impact | Detail |
|-------|------------|-------------|--------|--------|
| _awaiting first scan_ | | | | |

Detail files: `specs/fidelity/mocks/<trait-name>.md`

## ADR Enforcement

| ADR | Decision (short) | Status |
|-----|------------------|--------|
| _awaiting first scan_ | | |

Detail file: `specs/fidelity/adrs/enforcement.md`

## Cross-Cutting Gaps

See `specs/fidelity/gaps.md` for: dead specs, orphan tests, stale specs,
uncovered modules, untested feature-flag code.

## Priority Actions

_Populated after first scan._

## Changelog

| Date | Action | Delta |
|------|--------|-------|
| _awaiting first scan_ | | |
