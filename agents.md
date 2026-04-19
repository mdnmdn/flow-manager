# Flow Manager Agent Guide

## Overview
Flow Manager (`fm`) is a Rust-based CLI tool designed to automate and simplify the development workflow using Azure DevOps, Git, and SonarQube. It provides "porcelain" commands for high-level workflows and "plumbing" commands for low-level operations.

## Project Structure
- `src/cli/`: CLI command definitions using `clap`.
- `src/commands/`: Implementation of porcelain and plumbing commands.
- `src/core/`: Core business logic, context management, and domain models.
- `src/providers/`: Interfaces and implementations for external providers (starting with Azure DevOps).
- `_docs/`: Documentation and proposals.
    - [`command-specifications.md`](_docs/command-specifications.md): Summarizes expected commands and features.
    - [`component-specification.md`](_docs/component-specification.md): Breaks down high-level commands into low-level component requirements.
    - [`porcelain-commands-proposal.md`](_docs/porcelain-commands-proposal.md): Detailed proposal for high-level workflow commands.
    - [`project-structure.md`](_docs/project-structure.md): Overview of the project architecture and directory layout.
    - [`github-provider-analysis.md`](_docs/github-provider-analysis.md): Feasibility analysis for implementing a GitHub provider.
    - [`gitlab-provider-analysis.md`](_docs/gitlab-provider-analysis.md): Feasibility analysis for implementing a GitLab provider.
    - [`bitbucket-provider-analysis.md`](_docs/bitbucket-provider-analysis.md): Feasibility analysis for implementing a Bitbucket/Atlassian (Jira + Bitbucket) provider.

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
