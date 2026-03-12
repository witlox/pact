# pact — development task runner
# Install: cargo install just
# Usage:  just <recipe>

default:
    @just --list

# Type-check the workspace
check:
    cargo check --workspace

# Format all code
fmt:
    cargo fmt --all

# Check formatting (CI mode)
fmt-check:
    cargo fmt --all -- --check

# Run clippy lints (deny warnings)
lint:
    cargo clippy --workspace --all-targets -- -D warnings

# Run tests (skips tests marked #[ignore], excludes acceptance)
test:
    #!/usr/bin/env bash
    set -euo pipefail
    if command -v cargo-nextest &>/dev/null; then
        cargo nextest run --workspace --exclude pact-acceptance
    else
        cargo test --workspace --exclude pact-acceptance
    fi

# Run BDD acceptance tests (cucumber, custom harness)
test-accept:
    cargo test -p pact-acceptance

# Run the full test suite including slow tests
test-all:
    #!/usr/bin/env bash
    set -euo pipefail
    if command -v cargo-nextest &>/dev/null; then
        cargo nextest run --workspace --run-ignored all
    else
        cargo test --workspace -- --include-ignored
    fi

# Run only the slow (ignored) tests
test-slow:
    #!/usr/bin/env bash
    set -euo pipefail
    if command -v cargo-nextest &>/dev/null; then
        cargo nextest run --workspace --run-ignored ignored-only
    else
        cargo test --workspace -- --ignored
    fi

# Run cargo-deny checks
deny:
    cargo deny check

# Run advisory audit only
audit:
    cargo deny check advisories

# Build workspace
build:
    cargo build --workspace

# Run the fast CI suite locally
all: fmt-check lint test deny

# Run the full CI suite locally (all tests)
ci: fmt-check lint test-all deny

# Build release binaries
release:
    cargo build --workspace --release

# Run pact-agent in dev mode
run-agent:
    RUST_LOG=debug cargo run --package pact-agent -- --config config/minimal.toml

# Run pact-journal in dev mode
run-journal:
    RUST_LOG=debug cargo run --package pact-journal -- --config config/minimal.toml

# Run pact CLI with args
cli *ARGS:
    cargo run --package pact-cli -- {{ARGS}}

# Clean build artifacts
clean:
    cargo clean
