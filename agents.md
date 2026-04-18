# Flow Manager Agent Guide

## Overview
Flow Manager (`fm`) is a Rust-based CLI tool designed to automate and simplify the development workflow using Azure DevOps, Git, and SonarQube. It provides "porcelain" commands for high-level workflows and "plumbing" commands for low-level operations.

## Project Structure
- `src/cli/`: CLI command definitions using `clap`.
- `src/commands/`: Implementation of porcelain and plumbing commands.
- `src/core/`: Core business logic, context management, and domain models.
- `src/providers/`: Interfaces and implementations for external providers (starting with Azure DevOps).
- `_docs/`: Documentation and proposals.

## Key Concepts
- **Context:** The tool determines if you are in a "Baseline" context (shared branch) or "Activity" context (feature/fix branch linked to a Work Item).
- **Idempotency:** Commands are designed to be safe to run multiple times.
- **Submodule Support:** Transparent handling of the `_docs` submodule.

## Instructions for Agents
- When adding new commands, define them in `src/cli/` and implement the logic in `src/commands/`.
- Ensure all new features are provider-agnostic in `src/core/`, using traits defined in `src/providers/`.
- Maintain the idempotency of porcelain commands.
- Update `_docs/project-structure.md` if the architecture changes.
- Use `dotenvy` for environment variable management.

## Testing
- Rust tests should be placed in `src/` (unit tests) or `tests/` (integration tests).
- Mock providers should be used for testing core logic without hitting external APIs.
