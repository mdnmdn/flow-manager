# Flow Manager (fm) Command Specifications

This document summarizes the expected commands and features of the `fm` CLI tool. All commands follow a specific logic described in the pseudo-code within the source files.

## Global Options
- `--format`: Output format (default: `markdown`, also supports `json`).

## Context Commands
### `fm context`
- **Goal**: Snapshot of the current activity. Entry point for every new work session.
- **Implementation**: `src/commands/context.rs`

## Work Commands (`fm work ...`)
- **Implementation**: `src/commands/work/mod.rs`
### `fm work new`
- **Goal**: Start a new activity (WI, branch, draft PR).
### `fm work load <id>`
- **Goal**: Resume an existing activity, repairing context if necessary.
### `fm work list`
- **Goal**: List active work items.

## Task Commands (`fm task ...`)
- **Implementation**: `src/commands/task/mod.rs`
### `fm task hold`
- **Goal**: Pause current activity and return to baseline.
### `fm task update`
- **Goal**: Update the linked Work Item.
### `fm task sync`
- **Goal**: Sync activity branch with baseline using merge or rebase.
### `fm task complete`
- **Goal**: Finalize activity after PR is merged.

## PR Commands (`fm pr ...`)
- **Implementation**: `src/commands/pr/mod.rs`
### `fm pr show [<id>]`
- **Goal**: Display PR details.
### `fm pr update`
- **Goal**: Update PR fields or publish draft.
### `fm pr merge`
- **Goal**: Complete/merge the PR using configured strategy.
### `fm pr review <id>`
- **Goal**: Switch to another PR for review (auto-stashes current work).

## Todo Commands (`fm todo ...`)
- **Implementation**: `src/commands/todo/mod.rs`
- Manage child Tasks of the current User Story.
- `show`, `new`, `pick`, `complete`, `reopen`, `update`, `next`.

## Pipeline Commands (`fm pipeline ...`)
- **Implementation**: `src/commands/pipeline/mod.rs`
- `run`: Trigger CI for the current branch.
- `status`: Show/watch latest run status.

## Source Control Commands
### `fm commit`
- **Goal**: Commit with transparent `_docs` submodule handling.
- **Implementation**: `src/commands/commit.rs`
### `fm push`
- **Goal**: Push with transparent `_docs` submodule handling.
- **Implementation**: `src/commands/push.rs`
### `fm sync`
- **Goal**: `commit --all` + `push`.
- **Implementation**: `src/commands/sync.rs`

## Quality Commands
### `fm sonar`
- **Goal**: List SonarQube issues for the current project.
- **Implementation**: `src/commands/sonar.rs`

## Plumbing Commands
- **Implementation**: `src/commands/plumbing/`
- Low-level access to Git and ADO providers.
