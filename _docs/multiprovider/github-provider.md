# GitHub Provider Reference

## Overview

The GitHub provider (`GitHubProvider` in `src/providers/github.rs`) implements `IssueTracker`, `VCSProvider`, and `PipelineProvider` traits for GitHub and GitHub Enterprise. It uses the GitHub REST API v3 for all operations.

## Configuration

### Structure

```toml
[provider]
type = "github"

[provider.github]
token    = ""        # Personal Access Token or fine-grained token
owner    = ""        # GitHub organization or user
repo     = ""        # Repository name
base_url = ""        # Optional: GitHub Enterprise base URL (defaults to https://api.github.com)
```

### Authentication

The GitHub provider supports several authentication methods, resolved in the following priority:

1.  **Explicit Token (PAT)**: Set via the `token` field in `fm.toml` or the `FM__PROVIDER__GITHUB__TOKEN` environment variable.
2.  **GitHub Actions Token**: Automatically detected via the `GITHUB_TOKEN` environment variable in CI environments.
3.  **GitHub CLI Token**: Detected via the `GH_TOKEN` environment variable.
4.  **OS Keychain (Device Flow)**: Used when no explicit token is found. Requires authentication via `fm auth login`.

#### GitHub App / Device Flow

For local development, the recommended method is to use `fm auth login`. This initiates the GitHub OAuth Device Flow:
1.  Run `fm auth login`.
2.  Open the provided URL and enter the user code.
3.  The resulting User Access Token is stored securely in your OS keychain.
4.  Tokens are automatically refreshed if they expire (when using `load_valid_token`).

#### Multi-Account Support

`fm` supports multiple GitHub accounts. You can manage them using the `--account` flag:
- `fm auth login --account work`
- `fm auth status --account work`
- `fm auth list` (to see all stored accounts)

In your project's `fm.toml`, bind a specific account:
```toml
[provider.github]
account = "work"
```

#### Token Scopes (for PATs)

**Required classic token scopes:**
- `repo` — Issues, pull requests, branches
- `workflow` — GitHub Actions dispatch

**Fine-grained token permissions:**
- Issues (read/write)
- Pull Requests (read/write)
- Contents (read)
- Actions (read/write)

### Base URL Override

For GitHub Enterprise Server, set `base_url` to the enterprise instance URL (e.g., `https://github.company.com/api/v3`). The URL is normalized to remove trailing slashes.

## Capabilities

```rust
ProviderCapabilities {
    draft_pull_requests: true,
    pipeline_support: true,
    work_item_hierarchy: false,
    formal_artifact_links: false,
    merge_strategies: [Squash, Rebase, RebaseMerge, NoFastForward],
    work_item_relations: ["relates_to", "blocks"],
}
```

## IssueTracker Implementation

GitHub Issues are the work item model. Work item type is encoded in a label with `type:` prefix.

### Type Mapping

| Internal Type | Label |
|---------------|-------|
| `Bug` | `type:bug` |
| `Feature` | `type:feature` |
| `Task` | `type:task` |
| `User Story` | `type:user-story` |
| Other | `type:{lowercase}` |

If no `type:` label is found, the work item defaults to type `"Task"`.

### State Mapping

GitHub's binary state (`open`/`closed`) maps directly:
- `state="open"` ← active/in-progress states
- `state="closed"` ← completed/resolved states

`update_work_item_state("active")` → `state: "open"`; any other state → `state: "closed"`.
`available_states()` returns `["open", "closed"]`.

### Methods

| Method | Endpoint | Notes |
|--------|----------|-------|
| `get_work_item(id)` | `GET /repos/{owner}/{repo}/issues/{id}` | Parse GitHub issue number |
| `create_work_item(title, type, desc, assigned, tags)` | `POST /repos/{owner}/{repo}/issues` | Labels: `[type:X, ...tags]` |
| `update_work_item(id, title, desc, assigned, tags)` | `PATCH /repos/{owner}/{repo}/issues/{id}` | Partial update |
| `update_work_item_state(id, state)` | `PATCH /repos/{owner}/{repo}/issues/{id}` | `state: open/closed` |
| `query_work_items(filter)` | `GET /search/issues` | Builds GitHub search query from filter |
| `create_artifact_link(wi_id, url)` | `POST /repos/{owner}/{repo}/issues/{wi_id}/comments` | Posts comment: `"Branch: {url}"` |
| `link_work_items(src, tgt, relation)` | `POST /repos/{owner}/{repo}/issues/{src}/comments` | Posts comment: `"Relates to #{tgt}"` |
| `get_child_work_items(id, type)` | — | Returns empty list (not supported) |
| `get_work_item_comments(id)` | `GET /repos/{owner}/{repo}/issues/{id}/comments` | Issue comments only |
| `add_work_item_comment(id, text)` | `POST /repos/{owner}/{repo}/issues/{id}/comments` | Posts new comment |

### Search Query Translation

`WorkItemFilter` translates to GitHub search syntax:

| Filter Field | Query Term |
|--------------|-----------|
| `state` | `is:open` / `is:closed` |
| `assigned_to` | `assignee:{user}` |
| `labels` | `label:{l}` (one per entry) |
| `work_item_type` | `label:type:{type}` |
| `text` | `"{text}"` (quoted) |
| `milestone` | `milestone:{milestone}` |
| `limit` | Post-filter truncation |

All terms combined with space (AND logic). Example: `repo:owner/repo is:issue is:open assignee:bob label:type:bug` (open bugs assigned to bob).

### Known Limitations

- **No hierarchy:** `get_child_work_items()` returns empty list. `fm todo` reports "not supported". Parent-child relationships require GitHub Sub-issues API (beta) or markdown task list parsing (not yet implemented).
- **No formal links:** `create_artifact_link()` and `link_work_items()` post standardized comments. The API does not expose formal link objects.
- **Relation types ignored:** All cross-links posted as `"Relates to #{id}"` comments. The `relation` parameter is ignored.

## VCSProvider Implementation

Pull requests and branches. PR status mapping:

| GitHub State | FM Status |
|--------------|-----------|
| `open` | `"active"` |
| `closed` + `merged` | `"completed"` |
| `closed` + `unmerged` | `"abandoned"` |

Draft PRs set `is_draft: true`.

### Merge Strategy Mapping

| FM Strategy | GitHub `merge_method` |
|-------------|----------------------|
| `Squash` | `"squash"` |
| `Rebase` | `"rebase"` |
| `RebaseMerge` | `"rebase"` |
| `NoFastForward` | `"create"` |

### Methods

| Method | Endpoint | Notes |
|--------|----------|-------|
| `get_pull_request_by_branch(_, branch)` | `GET /repos/{owner}/{repo}/pulls?head={owner}:{branch}&state=open` | Return first match or None |
| `get_pull_request_details(_, id)` | `GET /repos/{owner}/{repo}/pulls/{id}` | Direct |
| `create_pull_request(_, source, target, title, desc, draft, wi_refs)` | `POST /repos/{owner}/{repo}/pulls` | WI refs appended to description as `"Related: #{id}"` |
| `update_pull_request(_, id, title, desc, draft, status)` | `PATCH /repos/{owner}/{repo}/pulls/{id}` | Partial update; status "completed"/"abandoned" → auto-merge |
| `complete_pull_request(_, id, strategy, delete_source)` | `PUT /repos/{owner}/{repo}/pulls/{id}/merge` | Merge with strategy; optionally delete source branch |
| `add_reviewer(_, id, reviewer_id)` | `POST /repos/{owner}/{repo}/pulls/{id}/requested_reviewers` | Request review from user |
| `create_branch(_, name, source)` | `GET /repos/{owner}/{repo}/git/ref/heads/{source}` → `POST /repos/{owner}/{repo}/git/refs` | Create ref from source branch's SHA |
| `delete_branch(_, name)` | `DELETE /repos/{owner}/{repo}/git/refs/heads/{name}` | Direct |
| `get_repository(_)` | `GET /repos/{owner}/{repo}` | Return repo ID, full name, default branch |

### Pull Request Comments and Threads

Threads are collected from two endpoints:

| Endpoint | Content |
|----------|---------|
| `GET /repos/{owner}/{repo}/pulls/{id}/comments` | Review comments (line-anchored to diffs) |
| `GET /repos/{owner}/{repo}/issues/{id}/comments` | General discussion comments |

Replies are grouped by `in_reply_to_id` field in review comments.

| Method | Endpoint | Notes |
|--------|----------|-------|
| `get_pull_request_threads(_, id)` | Line comments + general comments | Group replies by `in_reply_to_id` |
| `reply_to_pull_request_thread(_, pr_id, thread_id, msg)` | `POST /repos/{owner}/{repo}/pulls/{pr_id}/comments` | Set `in_reply_to: {thread_id}` |
| `update_pull_request_thread_status(_, pr_id, thread_id, status)` | — | **No-op** (logs warning). GraphQL not implemented. |
| `get_pull_request_changed_files(_, pr_id)` | `GET /repos/{owner}/{repo}/pulls/{pr_id}/files` | Returns path and change type (modified/added/removed) |
| `add_pull_request_thread(_, pr_id, content, file?, line?)` | `POST /repos/{owner}/{repo}/pulls/{pr_id}/reviews` or `/issues/{pr_id}/comments` | File-anchored → review; otherwise → issue comment |

### Local Git Operations

All local git methods return an error directing the caller to use `LocalGitProvider`:
- `get_current_branch`
- `checkout_branch`
- `push`, `pull`, `fetch`
- `commit`, `discard_local_changes`, `get_log`
- `merge`, `rebase`
- `get_status`, `stash_push`, `stash_pop`
- `check_submodule_status`, `update_submodule_pointer`

### Known Limitations

- **No thread resolution:** `update_pull_request_thread_status()` is a no-op. Resolving review threads requires GitHub's GraphQL API (`resolveReviewThread` mutation), which is not yet implemented.
- **Thread replies:** Review comment replies use GitHub's `in_reply_to_id` field. General discussion comments are flat (no reply structure).

## PipelineProvider Implementation

GitHub Actions workflows and runs.

### Status Mapping

| GitHub State | FM Status |
|--------------|-----------|
| `queued`, `in_progress` | `"inProgress"` |
| `completed` | `"completed"` |

| GitHub Conclusion | FM Result |
|-------------------|-----------|
| `success` | `Some("succeeded")` |
| `failure` | `Some("failed")` |
| (other) | `Some("{other}")` |
| (null) | `None` |

### Methods

| Method | Endpoint | Notes |
|--------|----------|-------|
| `list_pipelines()` | `GET /repos/{owner}/{repo}/actions/workflows` | List available workflows |
| `run_pipeline(id, branch)` | `POST /repos/{owner}/{repo}/actions/workflows/{id}/dispatches` with `ref: branch` | Dispatch run; poll latest run on branch |
| `get_latest_run(branch)` | `GET /repos/{owner}/{repo}/actions/runs?branch={branch}&per_page=1` | Return most recent run |
| `get_run_status(run_id)` | `GET /repos/{owner}/{repo}/actions/runs/{run_id}` | Fetch run by ID |

### Known Limitations

- **Workflow dispatch required:** `run_pipeline()` requires the target workflow to have a `workflow_dispatch` trigger in its YAML. Workflows without it will fail silently.
- **Poll latency:** After dispatch, there is a brief window before the run appears in the API. `run_pipeline()` does a best-effort lookup of the latest run immediately after dispatch; may return "unknown" if the run is not yet indexed.

## CI Environment Detection

GitHub Actions is detected when `GITHUB_ACTIONS=true`. The `CiEnvironment` struct is populated:

### PR Builds

When `GITHUB_EVENT_NAME=pull_request`:
- `branch` ← `GITHUB_HEAD_REF` (source branch name)
- `pr_id` ← parsed from `GITHUB_REF` (`refs/pull/{number}/merge`)
- `pr_target_branch` ← `GITHUB_BASE_REF` (target branch name)

### Push Builds

When `GITHUB_EVENT_NAME` is not `pull_request`:
- `branch` ← `GITHUB_REF` stripped of `refs/heads/` prefix
- `pr_id` ← `None`
- `pr_target_branch` ← `None`

### Config Auto-population

`Config::load()` checks for `GITHUB_REPOSITORY` (`owner/repo` format) and auto-populates `provider.github.owner` and `.repo` if those fields are empty. This allows minimal config in CI:

```toml
[provider]
type = "github"

[provider.github]
token = ""  # Set via env var
```

The owner and repo are inferred from the environment.

### Available Variables

| Variable | Content |
|----------|---------|
| `GITHUB_ACTIONS` | Always `"true"` in GitHub Actions |
| `GITHUB_REF` | `refs/heads/...` (push) or `refs/pull/{N}/merge` (PR) |
| `GITHUB_HEAD_REF` | Branch name in PR builds |
| `GITHUB_BASE_REF` | Target branch in PR builds |
| `GITHUB_EVENT_NAME` | Event type (`pull_request`, `push`, `workflow_dispatch`, etc.) |
| `GITHUB_RUN_ID` | Unique run identifier |
| `GITHUB_REPOSITORY` | `owner/repo` |

## Next Steps / Limitations

1. **GraphQL for thread resolution:** Implement `update_pull_request_thread_status()` via `POST /graphql` using `resolveReviewThread` mutation.
2. **Work item hierarchy:** Implement `get_child_work_items()` via markdown task list parsing (`- [ ] #N` in issue body) or GitHub Sub-issues API (beta).
3. **Plumbing commands:** Add `fm plumbing github` subcommand for debug operations (`issue_get`, `pr_get`, etc.).
4. **Formal link operations:** Consider GitHub GraphQL API for richer link metadata when available.
5. **Rate limit handling:** Add retry-after logic for `403 rate limit exceeded` responses.

## Troubleshooting

### "Not implemented for GitHub provider (use LocalGitProvider)"

Any local git operation (commit, push, branch checkout, etc.) raises this error. Use `LocalGitProvider` instead — it's the default for all git commands.

### "update_pull_request_thread_status: GraphQL not implemented for GitHub provider"

Thread status updates (resolve/reopen) log this warning and return success. The feature requires GitHub's GraphQL API, which is not yet implemented. Workaround: manually mark threads as resolved in the GitHub UI, or add a follow-up comment to signal resolution.

### "Missing [provider.github] config"

Ensure `fm.toml` has a `[provider.github]` section with `token`, `owner`, and `repo` filled in. Or use `fm init --discover` to auto-populate from the git remote.

### "No runs" when dispatching a workflow

Ensure the workflow file has a `workflow_dispatch` trigger. The run must be manually triggered via the dispatch event; default `push` triggers will not appear in the dispatch API.
