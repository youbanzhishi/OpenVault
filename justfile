# OpenVault — justfile

set dotenv-load

# Default: build everything
default: build

# Build the workspace
build:
    CARGO_TARGET_DIR=/tmp/openvault-target cargo build --workspace

# Run all tests
test:
    CARGO_TARGET_DIR=/tmp/openvault-target cargo test --workspace

# Run tests with output
test-verbose:
    CARGO_TARGET_DIR=/tmp/openvault-target cargo test --workspace -- --nocapture

# Run the CLI
run *ARGS:
    CARGO_TARGET_DIR=/tmp/openvault-target cargo run --bin vault -- {{ARGS}}

# Check without building
check:
    CARGO_TARGET_DIR=/tmp/openvault-target cargo check --workspace

# Format code
fmt:
    cargo fmt --all

# Lint
clippy:
    CARGO_TARGET_DIR=/tmp/openvault-target cargo clippy --workspace -- -D warnings

# Clean build artifacts
clean:
    CARGO_TARGET_DIR=/tmp/openvault-target cargo clean

# Full CI check
ci: fmt clippy test
