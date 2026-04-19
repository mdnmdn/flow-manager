# Component Specification

This document breaks down the high-level commands into low-level features and function requirements for each component.

## 1. VCS Provider (`src/providers/mod.rs` — `VCSProvider` trait) [COMPLETED]

### Remote operations (implemented by `adonet.rs`)
- `create_branch(repository, name, source)`: Create a remote branch from a source ref.
- `delete_branch(repository, name)`: Remove a remote branch.
- `get_repository(name)`: Fetch repository metadata.
- `get_pull_request_by_branch(repository, branch)`: Find the PR for a branch.
- `get_pull_request_details(repository, id)`: Fetch full PR details (reviewers, status, …).
- `create_pull_request(repository, source, target, title, description, is_draft, work_item_refs)`: Create a PR, linking work items at creation time.
- `update_pull_request(repository, id, title, description, is_draft, status)`: Update PR fields.
- `complete_pull_request(repository, id, strategy, delete_source_branch)`: Merge a PR.
- `add_reviewer(repository, id, reviewer_id)`: Add a reviewer to a PR.

### Local git operations (implemented by `git.rs` — `LocalGitProvider`)
- `get_current_branch()`: Return the active local branch name.
- `checkout_branch(name)`: Switch to a local/remote branch.
- `get_status()`: Return short `git status` output.
- `get_log(range, limit)`: Fetch commit history.
- `stash_push(message)`: Stash all working-tree changes with a label.
- `stash_pop()`: Pop the top stash.
- `merge(source)`: Merge a branch into current.
- `rebase(target)`: Rebase current branch onto target.
- `push(force)`: Push to remote (`--force-with-lease` when `force = true`).
- `pull()`: Pull from remote.
- `fetch()`: `git fetch origin`.
- `commit(message, all, amend)`: Create or amend a local commit.
- `discard_local_changes()`: `git checkout -- .`
- `check_submodule_status(path)`: Returns `true` if the submodule has uncommitted or unpushed changes.
- `update_submodule_pointer(path)`: Stage the updated submodule pointer (`git add <path>`).

### LocalGitProvider utility methods (not on trait)
- `get_repo_name()`: Derive repository name from `git remote get-url origin`.
- `find_branch_for_wi(wi_id)`: Scan `git branch -r` for any branch matching `/<wi_id>-`.
- `has_staged_changes()`: Return `true` if `git diff --cached` is non-empty.
- `stash_push_staged(message)`: Stash only staged (index) changes via `git stash push --staged`.
- `stash_pop_named(name, restore_index)`: Find a stash by label and pop it; `restore_index = true` re-stages via `--index`.
- `run_git(args)`: Execute an arbitrary git subprocess and return stdout.

## 2. Issue Tracker (`src/providers/mod.rs` — `IssueTracker` trait) [COMPLETED]

- `get_work_item(id)`: Fetch details of a specific WI.
- `create_work_item(title, type, description, assigned_to, tags)`: Create a new WI.
- `update_work_item(id, title, description, assigned_to, tags)`: Update WI fields.
- `update_work_item_state(id, state)`: Transition a WI to a new state (Active, Closed, …).
- `query_work_items(filter)`: Query WIs with type, state, assignee, and limit filters.
- `create_artifact_link(wi_id, url)`: Link a branch or PR URL to a WI.
- `link_work_items(source_id, target_id, relation)`: Create a parent/child or other relation.
- `get_child_work_items(id, type)`: List child WIs (e.g. Tasks under a User Story).
- `available_states(id)`: Return valid next states for a WI (default: empty vec).

## 3. Pipeline Provider (`src/providers/mod.rs` — `PipelineProvider` trait) [COMPLETED]

- `list_pipelines()`: List available pipeline definitions.
- `run_pipeline(pipeline_id, branch)`: Trigger a new run.
- `get_latest_run(branch)`: Fetch the most recent run for a branch.
- `get_run_status(run_id)`: Get detailed status/result of a specific run.

## 4. Quality Provider (`src/providers/mod.rs` — `QualityProvider` trait) [COMPLETED]

- `get_open_issues(project_key, severity)`: Fetch open issues from SonarQube for a project.

## 5. Provider Factory (`src/providers/factory.rs`) [COMPLETED]

- `ProviderSet::from_config(config)`: Reads `config.provider.kind` (`"ado"`, `"github"`, `"gitlab"`) and constructs `issue_tracker`, `vcs`, and optional `pipeline` provider instances.

## 6. Config (`src/core/config.rs`) [COMPLETED]

- Load from `fm.toml`, `fm.yaml`, environment variables, or `.env` file (via `dotenvy`).
- `ProviderConfig` struct with `kind: String` field and optional sub-configs (`ado`, `github`, `gitlab`).
- `FmConfig` struct: `default_target`, `merge_strategy`, `submodules`.
- Optional `SonarConfig` section.

## 7. Context and Output (`src/core/context.rs`) [COMPLETED]

- `ContextManager::detect(branch)`: Returns `Context::Baseline` or `Context::Activity { wi_id, branch }`. When the branch name does not match the `feature/{id}-slug` regex, falls back to `BranchCache::load_for_branch` before returning `Baseline`.
- `ContextManager::resolve_id(input)`: Disambiguates `w-123`, `pr-123`, plain numbers, and branch names into `IdResolution` variants.
- `ContextManager::derive_branch_name(wi_id, title, type)`: Produces `feature/{id}-{slug}` or `fix/{id}-{slug}`.
- `OutputFormatter::format(data, format, template)`: Renders a struct as Markdown (via Handlebars template) or JSON.

## 8. Branch Cache (`src/core/branch_cache.rs`) [COMPLETED]

Provides a lightweight per-repository hint file so that branches with non-conventional names are still recognized as Activity context.

**Cache file location:** `$TMPDIR/fm_branch_{hash}.json`
The `{hash}` is an FNV-1a 64-bit hash of the git repository root path, scoping the cache to the current repo.

**Data stored:**
```json
{ "branch": "some-branch", "wi_id": "12345", "wi_type": "feature" }
```

**API:**
- `BranchCache::save(branch, wi_id, wi_type)`: Write (or overwrite) the cache for the current repo.
- `BranchCache::load_for_branch(branch) -> Option<BranchCache>`: Read the cache and return it only if the stored branch matches the given branch exactly and `wi_id` is non-empty; returns `None` otherwise (stale or missing).
- `BranchCache::clear()`: Delete the cache file (silently ignores missing file).

**Lifecycle (managed by commands):**

| Command | Action |
|---------|--------|
| `fm task new` — after checkout | `save` |
| `fm task load` — after checkout | `save` |
| `fm task hold` — before baseline switch | `clear` |
| `fm task complete` — before baseline switch | `clear` |

## 9. Internal Coordination (commands layer)

- **ID disambiguation:** `ContextManager::resolve_id` covers WI IDs, PR IDs, branch names, and ambiguous plain numbers.
- **Stash lifecycle:** `fm task hold` auto-stashes by default — `stash-{wi-id}-staged` (index only via `--staged`) and `stash-{wi-id}-unstaged` (remaining working-tree changes); `--force` discards instead. `fm task load` restores both in order, re-staging the staged stash with `--index`.
- **Idempotency:** state-creating operations (`create_branch`, `create_pull_request`, `create_work_item`) check for existing resources before creating; duplicate links and state transitions are silently skipped.
- **Submodule transparency:** `fm commit`, `fm push`, and `fm sync` detect pending `_docs` changes and commit/push the submodule before the parent repo.
- **Todo resolution:** `fm todo` commands accept a numeric task ID or a case-insensitive title fragment scoped to the current WI's children.
