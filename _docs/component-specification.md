# Component Specification

This document breaks down the high-level commands into low-level features and function requirements for each component.

## 1. VCS Provider (Git / Azure DevOps Git) [COMPLETED]

The VCS provider handles local and remote repository operations.

### Features
- **Branch Management**:
    - [x] `get_current_branch()`: Identify the active local branch.
    - [x] `create_branch(name, target)`: Create a remote branch from a target baseline.
    - [x] `checkout_branch(name)`: Fetch and switch to a local/remote branch.
    - [x] `delete_remote_branch(name)`: Remove a branch from the remote.
- **Pull Request Management**:
    - [x] `get_pull_request(branch)`: Find the PR associated with a specific branch.
    - [x] `get_pull_request_details(id)`: Fetch full details (reviewers, status, etc.).
    - [x] `create_pull_request(source, target, title, description, is_draft)`: Create a new PR.
    - [x] `update_pull_request(id, fields)`: Update PR fields (status, draft, title, description).
    - [x] `complete_pull_request(id, strategy, delete_source)`: Merge a PR using a specific strategy.
    - [x] `add_reviewer(id, email)`: Add a reviewer to a PR.
- **Git Operations**:
    - [x] `get_status()`: Check for uncommitted changes and ahead/behind counts.
    - [ ] `get_log(range, limit)`: Fetch commit history. (Deferred)
    - [x] `stash_push(message)`: Stash uncommitted changes.
    - [x] `stash_pop()`: Restore a stash matching a criteria.
    - [ ] `merge(source)`: Merge a branch into current. (Deferred)
    - [ ] `rebase(target)`: Rebase current branch onto target. (Deferred)
    - [x] `push(force_with_lease)`: Push commits to remote.
    - [x] `pull()`: Update current branch from remote.
    - [x] `commit(message, all)`: Create a commit locally.
- **Submodule Support**:
    - [x] `check_submodule_status(path)`: Check for changes/unpushed commits in a submodule.
    - [x] `update_submodule_pointer(path)`: Stage and commit a submodule update.

## 2. Issue Tracker (Azure DevOps Work Items) [COMPLETED]

The Issue Tracker manages work items and their relationships.

### Features
- **Work Item Management**:
    - [x] `get_work_item(id)`: Fetch details of a specific WI.
    - [x] `create_work_item(title, type, description, assigned_to, tags)`: Create a new WI.
    - [x] `update_work_item(id, fields)`: Update WI fields (state, title, description, etc.).
    - [x] `update_work_item_state(id, state)`: Transition a WI to a new state (e.g., Active, Closed).
    - [x] `query_work_items(wiql)`: Execute a raw WIQL query for filtering.
- **Link Management**:
    - [x] `create_artifact_link(wi_id, url)`: Link a branch or PR to a WI.
    - [x] `link_work_items(source_id, target_id, relation)`: Create a parent/child or other relationship.
    - [x] `get_child_work_items(id, type)`: List children of a specific WI (e.g., todos of a story).

## 3. Pipeline Provider (Azure DevOps Pipelines) [COMPLETED]

The Pipeline Provider handles CI/CD interactions.

### Features
- **Run Management**:
    - [x] `run_pipeline(definition_id, branch)`: Trigger a new run.
    - [x] `get_latest_run(branch)`: Fetch the most recent run for a branch.
    - [x] `get_run_status(run_id)`: Get detailed status/results of a specific run.
- **Discovery**:
    - [x] `list_pipelines()`: List available pipeline definitions for auto-detection.

## 4. Quality Provider (SonarQube) [COMPLETED]

Handles integration with code quality tools.

### Features
- [x] `get_open_issues(project_key, severity)`: Fetch a list of open issues for a project.

## 5. Internal Logic / Coordination

These are high-level "low-level" steps implemented in the commands.

- **ID Disambiguation**:
    - `resolve_id(input)`: Logic to determine if an input is a WI ID, PR ID, or branch name.
- **Context Management**:
    - `parse_wi_id_from_branch(branch_name)`: Extract the numeric ID from `feature/{id}-slug`.
    - `validate_activity_invariants()`: Ensure WI, branch, and PR links are consistent.
- **Idempotency Logic**:
    - `ensure_resource_exists(resource_type, identifier, create_fn)`: Generic logic to reuse existing resources or create them if missing.
- **Todo Resolution**:
    - `resolve_todo_reference(parent_id, reference)`: Substring match or ID lookup for child tasks.
- **Template Rendering**:
    - `render_markdown(template, data)`: Format provider data into the standard FM markdown output.
