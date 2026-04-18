# Flow Manager (fm) Command Specifications

This document summarizes the expected commands and features of the `fm` CLI tool.

## Global Options
- `--format`: Output format (default: `markdown`, also supports `json`).

## Context Commands
### `fm context`
- **Goal**: Snapshot of the current activity.
- **Baseline branch**: Shows branch name and last 5 commits.
- **Activity branch**: Extracts WI ID from branch, fetches WI details, PR status, Git status (ahead/behind), and latest CI run.

## Work Commands (`fm work ...`)
### `fm work new`
- **Goal**: Start a new activity (WI, branch, draft PR).
- **Features**: Idempotent creation, Sonar issues integration in description.
### `fm work load <id>`
- **Goal**: Resume an existing activity.
- **Features**: Context repair (ensures branch/PR/links exist), auto-stash restoration.
### `fm work list`
- **Goal**: List active work items.
- **Features**: Filters for current user, state, and type.

## Task Commands (`fm task ...`)
### `fm task hold`
- **Goal**: Pause current activity and return to baseline.
- **Features**: Auto-stash option, push before switching.
### `fm task update`
- **Goal**: Update the linked Work Item.
### `fm task sync`
- **Goal**: Sync activity branch with baseline.
- **Features**: Merge (default) or Rebase, dry-run check.
### `fm task complete`
- **Goal**: Finalize activity after PR is merged.

## PR Commands (`fm pr ...`)
### `fm pr show [<id>]`
- **Goal**: Display PR details.
### `fm pr update`
- **Goal**: Update PR fields or publish draft.
### `fm pr merge`
- **Goal**: Complete/merge the PR using configured strategy.
### `fm pr review <id>`
- **Goal**: Switch to another PR for review (auto-stashes current work).

## Todo Commands (`fm todo ...`)
- Manage child Tasks of the current User Story.
- `show`: List todos.
- `new`: Create a new child Task.
- `pick`: Mark a todo as Active.
- `complete`: Mark a todo as Closed.
- `reopen`: Re-open a todo.
- `update`: Update todo details.
- `next`: Show/pick the next unstarted todo.

## Pipeline Commands (`fm pipeline ...`)
- `run`: Trigger CI for the current branch.
- `status`: Show/watch latest run status.

## Source Control Commands
### `fm commit`
- **Goal**: Commit with transparent `_docs` submodule handling.
### `fm push`
- **Goal**: Push with transparent `_docs` submodule handling.
### `fm sync`
- **Goal**: `commit --all` + `push`.

## Quality Commands
### `fm sonar`
- **Goal**: List SonarQube issues for the current project.

## Plumbing Commands
- Low-level access to Git and ADO providers for debugging or complex scenarios.
