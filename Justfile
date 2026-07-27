# Minimum supported Rust version; must match `rust-version` in Cargo.toml.
MSRV := "1.88"

_default:
    @just --list

# Build the project
build:
    cargo build

# Run the project
run *ARGS:
    cargo run -- {{ ARGS }}

# Open the viewer on the sample fixture
demo:
    cargo run -- tests/fixtures/sample.csv

# Build with optimizations
release:
    cargo build --release

# Install binary to user path
install:
    cargo install --path .

# Run tests
test:
    cargo test --quiet

# Run clippy linter
clippy:
    cargo clippy --all-targets -- -D warnings

# Run clippy and fix possible errors
clippy-fix:
    cargo clippy --all-targets --fix --allow-dirty --allow-staged 2>/dev/null

# Run rustfmt checker
fmt-check:
    cargo fmt -- --check

# Format code
fmt:
    cargo fmt

# Run all lints
lint: clippy fmt-check

# Build and test on the MSRV toolchain, as CI does
msrv:
    cargo +{{ MSRV }} build --locked --all-targets
    cargo +{{ MSRV }} test --quiet

# Everything CI runs. Use this before pushing: `just fix` uses your default
# toolchain and will not catch MSRV-only failures.
ci: lint test msrv

# Check all: lint and test
check-all: lint test

# Fix formatting and clippy warnings, then run tests
fix: fmt clippy-fix clippy test
