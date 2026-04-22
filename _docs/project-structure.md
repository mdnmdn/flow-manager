# Project Structure: Flow Manager (Rust)

## Overview

The Flow Manager (`fm`) is a CLI tool designed to streamline developer workflows by orchestrating operations across version control (Git), work item tracking (Azure DevOps or GitHub), and code quality tools (SonarQube). It follows a layered architecture to separate user-facing "porcelain" commands from low-level "plumbing" commands and infrastructure-specific providers.

## Directory Structure

```text
.
├── Cargo.toml
├── src/
│   ├── main.rs                   # CLI entry point, subcommand routing
│   ├── lib.rs                    # Library entry point (re-exports cli, commands, core, providers)
│   ├── cli/
│   │   └── mod.rs                # Full CLI definition using clap (Commands, TaskCommands, PrCommands, …)
│   ├── commands/                 # Command implementations organised by root command
│   │   ├── mod.rs
│   │   ├── context.rs            # fm context
│   │   ├── commit.rs             # fm commit
│   │   ├── push.rs               # fm push
│   │   ├── sync.rs               # fm sync
│   │   ├── sonar.rs              # fm sonar
│   │   ├── doctor.rs             # fm doctor
│   │   ├── init.rs               # fm init [--discover]
│   │   ├── work/
│   │   │   └── mod.rs            # fm task new / load / list / show (implementation)
│   │   ├── task/
│   │   │   └── mod.rs            # fm task hold / update / sync / complete / comment
│   │   ├── pr/
│   │   │   └── mod.rs            # fm pr show / update / merge / review
│   │   ├── todo/
│   │   │   └── mod.rs            # fm todo show / new / pick / complete / reopen / update / next
│   │   ├── pipeline/
│   │   │   └── mod.rs            # fm pipeline run / status
│   │   └── plumbing/
│   │       ├── mod.rs
│   │       ├── git.rs            # fm plumbing git …
│   │       └── ado.rs            # fm plumbing ado …
│   ├── core/                     # Core business logic and shared models
│   │   ├── mod.rs
│   │   ├── config.rs             # Config loading (TOML/YAML/env via `config` crate)
│   │   ├── context.rs            # Context detection, ID resolution, branch derivation, output formatting
│   │   ├── models.rs             # Domain entities: WorkItem, PullRequest, Pipeline, …
│   │   └── error.rs              # Shared error types
│   └── providers/                # Traits and implementations for external services
│       ├── mod.rs                # IssueTracker, VCSProvider, PipelineProvider, QualityProvider traits
│       ├── factory.rs            # ProviderSet: builds concrete providers from Config
│       ├── adonet.rs             # Azure DevOps REST API (issue tracker + VCS + pipeline)
│       ├── github.rs             # GitHub REST API (issue tracker + VCS + pipeline)
│       ├── git.rs                # LocalGitProvider: local git operations via subprocess
│       └── sonar.rs              # SonarQube API client
├── _docs/                        # Project documentation
│   ├── flow-manager-behaviours.md          # authoritative command reference
│   ├── component-specification.md
│   ├── config-structure.md
│   ├── project-structure.md      # this file
│   ├── provider-evolutions-extensibilities.md
│   └── multiprovider/
│       ├── github-provider.md              # GitHub provider reference (implemented)
│       ├── gitlab-provider-analysis.md     # GitLab feasibility analysis
│       └── bitbucket-provider-analysis.md  # Bitbucket feasibility analysis
└── AGENTS.md                     # Agent instructions and project overview
```

## Architectural Layers

### 1. CLI Layer (`src/cli/`)

Uses `clap` derive macros to define the full command-line interface in a single `mod.rs`.

- **Porcelain commands:** `Task`, `Pr`, `Todo`, `Pipeline`, `Context`, `Commit`, `Push`, `Sync`, `Sonar`, `Doctor`, `Init`
- **Plumbing commands:** nested under `Plumbing` — direct access to Git and ADO primitives

All `fm task new/load/list/show` (work item lifecycle) and `fm task hold/update/sync/complete/comment` (activity lifecycle) are routed through the same `Task` subcommand.

### 2. Command Layer (`src/commands/`)

Implements the logic for each CLI command. `main.rs` dispatches into these functions; they orchestrate provider calls and format output.

- `work/mod.rs` implements `new`, `load`, `list`, `show` (routed from `fm task new/load/list/show`)
- `task/mod.rs` implements `hold`, `update`, `sync`, `complete`, `comment`
- All other commands have a dedicated file or subdirectory

### 3. Core Layer (`src/core/`)

The "brain" of the application — provider-agnostic logic.

- **`config.rs`:** loads `fm.toml` / `fm.yaml` / env vars via the `config` crate; `ProviderConfig` uses a plain struct with a `kind` field (`"ado"`, `"github"`, `"gitlab"`) and optional sub-configs
- **`context.rs`:** derives Baseline vs. Activity context from the branch name; resolves ambiguous IDs (`w-123`, `pr-123`, plain numbers); slugifies titles for branch names; formats output via Handlebars templates
- **`models.rs`:** shared domain structs (`WorkItem`, `PullRequest`, `Pipeline`, `PipelineRun`, `QualityIssue`, …)

### 4. Provider Layer (`src/providers/`)

Handles communication with external systems behind shared traits.

- **`mod.rs`** defines: `IssueTracker`, `VCSProvider`, `PipelineProvider`, `QualityProvider`
- **`factory.rs`** (`ProviderSet`): reads `Config.provider.kind` and constructs the concrete provider set
- **`adonet.rs`**: Azure DevOps implementation of `IssueTracker`, `VCSProvider`, and `PipelineProvider`
- **`github.rs`**: GitHub REST API implementation of `IssueTracker`, `VCSProvider`, and `PipelineProvider`; maps issues→work items (via `type:` labels), PRs→pull requests, Actions workflows→pipelines
- **`git.rs`** (`LocalGitProvider`): implements `VCSProvider` local operations (checkout, stash, push, fetch, …) plus utility methods not on the trait: `get_repo_name()`, `find_branch_for_wi()`, `has_staged_changes()`, `stash_push_staged()`, `stash_pop_named()`
- **`sonar.rs`**: implements `QualityProvider` against SonarQube REST API

## Design Principles

- **Idempotency:** every command can be safely re-run; state-creating operations check for existing state first
- **Provider-agnostic core:** `src/core/` has no dependency on ADO-specific types
- **Transparent submodule handling:** `fm commit`, `fm push`, `fm sync` detect and handle the `_docs` submodule automatically
- **Non-interactive:** designed for humans, AI agents, and CI scripts; exits non-zero with structured messages on error
- **Dual-stash hold/restore:** `fm task hold --stash` preserves staged and unstaged changes as separate named stashes (`stash-{wi-id}-staged`, `stash-{wi-id}-unstaged`); `fm task load` restores them in the correct positions
