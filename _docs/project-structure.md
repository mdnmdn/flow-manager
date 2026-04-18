# Project Structure: Flow Manager (Rust)

## Overview

The Flow Manager (`fm`) is a CLI tool designed to streamline developer workflows by orchestrating operations across version control (Git), work item tracking (Azure DevOps), and code quality tools (SonarQube). It follows a layered architecture to separate user-facing "porcelain" commands from low-level "plumbing" commands and infrastructure-specific providers.

## Directory Structure

```text
.
├── Cargo.toml
├── src/
│   ├── main.rs          # CLI entry point, handles global flags and subcommand routing
│   ├── cli/             # CLI definition using clap
│   │   ├── mod.rs
│   │   ├── porcelain.rs # User-facing command definitions
│   │   └── plumbing.rs  # Low-level command definitions
│   ├── commands/        # Command implementations organized by root command
│   │   ├── mod.rs
│   │   ├── work/        # fm work ...
│   │   ├── task/        # fm task ...
│   │   ├── pr/          # fm pr ...
│   │   ├── todo/        # fm todo ...
│   │   ├── pipeline/    # fm pipeline ...
│   │   └── plumbing/    # low-level primitives
│   ├── core/            # Core business logic and shared models
│   │   ├── mod.rs
│   │   ├── context.rs   # Workspace/Activity context management
│   │   ├── models.rs    # Domain entities (WorkItem, PullRequest, etc.)
│   │   └── error.rs     # Error handling
│   ├── providers/       # Interface and implementations for external services
│   │   ├── mod.rs
│   │   ├── adonet.rs    # Azure DevOps REST API client
│   │   ├── git.rs       # Local Git command wrapper
│   │   └── sonar.rs     # SonarQube API client
│   └── lib.rs           # Library entry point
├── _docs/               # Project documentation
│   ├── porcelain-commands-proposal.md
│   └── project-structure.md
└── agents.md            # Agent instructions and project overview
```

## Architectural Layers

### 1. CLI Layer (`src/cli/`)
Uses `clap` to define the command-line interface.
- **Porcelain Commands:** High-level commands like `fm work new`, `fm sync`, `fm task hold`. These focus on developer intent.
- **Plumbing Commands:** Low-level commands that expose raw provider capabilities. Useful for debugging or scripts.

### 2. Command Layer (`src/commands/`)
Implements the logic for each CLI command, organized into subdirectories matching the main command groups.
- High-level commands orchestrate multiple provider calls.
- Ensures idempotency as described in the proposal.

### 3. Core Layer (`src/core/`)
Contains the "brain" of the application.
- **Context Model:** Derives the current state (Baseline vs. Activity) from the environment (branch name, ADO links).
- **Domain Models:** Shared data structures that represent work items, PRs, etc., independent of the provider's raw JSON format.

### 4. Provider Layer (`src/providers/`)
Handles communication with external systems.
- Defines traits (interfaces) for services like `IssueTracker`, `VCSProvider`, `PipelineManager`.
- Currently focuses on **Azure DevOps** as the primary provider, but designed to be extensible for GitHub, GitLab, etc.

## Design Principles

- **Idempotency:** Every command can be safely re-run.
- **Provider Agnostic Core:** The core logic should not depend on Azure DevOps-specific details where possible.
- **Transparent Submodule Handling:** Automatic management of the `_docs` submodule.
- **Non-Interactive:** Designed for both humans and AI agents.
