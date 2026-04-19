# Justfile for Flow Manager
# Common development tasks for the fm CLI tool

# Build the project
build:
	cargo build

# Run tests
test:
	cargo test

# Run tests with verbose output
test-v:
	cargo test --verbose

# Format code according to Rust standards
fmt:
	cargo fmt --all

# Check code style with clippy
lint:
	cargo clippy -- -D warnings

# Run the CLI tool with arguments
run:
	cargo run -- {{args}}

# Run the CLI tool with no arguments (shows help)
run-help:
	cargo run -- --help

# Clean build artifacts
clean:
	cargo clean

# Check for outdated dependencies
outdated:
	cargo outdated

# Update dependencies
update:
	cargo update

# Run all checks (fmt, lint, test)
check: fmt lint test

# CI check: verify formatting and clippy (no changes)
ci-check:
	cargo fmt --all -- --check
	cargo clippy -- -D warnings

# Watch for changes and run tests (requires cargo-watch)
watch-test:
	cargo watch -x test

# Watch for changes and run clippy
watch-lint:
	cargo watch -x clippy

# Initialize development environment
init:
	@echo "Setting up development environment..."
	@echo "Running: cargo fetch"
	cargo fetch
	@echo "Running: rustup component add rustfmt"
	rustup component add rustfmt
	@echo "Running: rustup component add clippy"
	rustup component add clippy
	@echo "Development environment ready!"

# Run documentation submodule commands
docs-update:
	git submodule update --init --recursive
	git submodule foreach git pull origin main

docs-status:
	git submodule status

.PHONY: build test test-v fmt lint run run-help clean outdated update check watch-test watch-lint init docs-update docs-status ci-check