# Flow Manager Agent Guide

## Overview
Flow Manager (`fm`) is a Rust-based CLI tool designed to automate and simplify the development workflow using Azure DevOps, Git, and SonarQube. It provides "porcelain" commands for high-level workflows and "plumbing" commands for low-level operations.

## Project Structure
- `src/cli/`: CLI command definitions using `clap`.
- `src/commands/`: Implementation of porcelain and plumbing commands.
- `src/core/`: Core business logic, context management, and domain models.
- `src/providers/`: Interfaces and implementations for external providers (starting with Azure DevOps).
- `_docs/`: Documentation.
    - [`flow-manager-behaviours.md`](_docs/flow-manager-behaviours.md): Authoritative reference for every command — steps, output, and behaviour.
    - [`component-specification.md`](_docs/component-specification.md): Provider traits, LocalGitProvider utilities, and internal coordination.
    - [`project-structure.md`](_docs/project-structure.md): Architecture, directory layout, and design principles.
    - [`config-structure.md`](_docs/config-structure.md): Full config reference and environment variable mapping.
    - [`github-provider-analysis.md`](_docs/multiprovider/github-provider-analysis.md): Feasibility analysis for a GitHub provider.
    - [`gitlab-provider-analysis.md`](_docs/multiprovider/gitlab-provider-analysis.md): Feasibility analysis for a GitLab provider.
    - [`bitbucket-provider-analysis.md`](_docs/multiprovider/bitbucket-provider-analysis.md): Feasibility analysis for a Bitbucket/Atlassian provider.

## Key Concepts
- **Context:** The tool determines if you are in a "Baseline" context (shared branch) or "Activity" context (feature/fix branch linked to a Work Item).
- **Idempotency:** Commands are designed to be safe to run multiple times.
- **Submodule Support:** Transparent handling of the `_docs` submodule.

## Instructions for Agents
- When adding new commands, define them in `src/cli/` and implement the logic in `src/commands/`.
- Before submitting a PR, always run:
    - `cargo fmt --all` to ensure consistent code style.
    - `cargo clippy -- -D warnings` to check for lints and common issues.
    - `cargo test` to ensure all tests pass.
- Ensure all new features are provider-agnostic in `src/core/`, using traits defined in `src/providers/`.
- Maintain the idempotency of porcelain commands.
- Update `_docs/project-structure.md` if the architecture changes.
- Use `dotenvy` for environment variable management.

## Testing
- Rust tests should be placed in `src/` (unit tests) or `tests/` (integration tests).
- Mock providers should be used for testing core logic without hitting external APIs.
