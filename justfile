default: check

build:
    cargo build --workspace

test:
    cargo test --workspace

test-all:
    cargo test --workspace -- --include-ignored

test-slow:
    cargo test --workspace -- --ignored

fmt-check:
    cargo fmt --all -- --check

clippy:
    cargo clippy --workspace --all-targets -- -D warnings

deny:
    cargo deny check

check: fmt-check clippy deny test

ci: fmt-check clippy deny test-all

fmt:
    cargo fmt --all

release:
    cargo build --workspace --release

run-agent:
    RUST_LOG=debug cargo run --package pact-agent -- --config config/minimal.toml

run-journal:
    RUST_LOG=debug cargo run --package pact-journal -- --config config/minimal.toml

cli *ARGS:
    cargo run --package pact-cli -- {{ARGS}}

clean:
    cargo clean
