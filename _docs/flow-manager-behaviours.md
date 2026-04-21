# Flow Manager — Behaviour Reference

This document is the authoritative description of every `fm` command: what it does, the steps it performs, and the output it produces. It reflects the current implementation.

---

## 1. Concepts

### Context model

At any point the developer is in one of two contexts:

| Context | Description |
|---------|-------------|
| **Baseline** | On a protected/shared branch (`main`, `develop`). No active work item. |
| **Activity** | On a `feature/*` or `fix/*` branch. Linked to a WI, a remote branch, and a draft PR. |

The **current branch is the source of truth** for the active context. `fm` reads the branch name to derive the work item ID and activity type. For branches that do not follow the naming convention, `fm` falls back to a [branch cache](#branch-cache).

### Branch naming

```
feature/{wi-id}-{short-slug}    ← User Story (feature)
fix/{wi-id}-{short-slug}        ← Bug
```

Examples:
```
feature/73235-pipeline-list-workflow-script
fix/73100-login-redirect-loop
```

`{wi-id}` is the work item ID from the issue tracker. `{short-slug}` is kebab-case derived from the WI title or supplied via `--branch`.

### Activity invariants

When in **Activity** context, the following must always be true:

1. A WI exists and is **Active**.
2. A remote branch exists (same name as local).
3. A **draft or active PR** exists, linked to the WI and targeting the baseline branch.
4. The WI links record both the branch and the PR as artifacts.

`fm task new` and `fm task load` both enforce and repair these invariants.

### Branch cache

When a work item is loaded whose linked branch does not follow the `feature/{id}-slug` / `fix/{id}-slug` convention (e.g. a branch created externally), `fm` cannot derive the WI ID from the branch name alone. To handle this, `fm task load` and `fm task new` write a small JSON hint to a per-repository temporary file:

```
$TMPDIR/fm_branch_{repo-hash}.json
```

The file stores the exact branch name, WI ID, and activity type (`feature` or `fix`). `ContextManager::detect` reads it as a fallback after the regex miss, accepting it only when the stored branch name matches the current branch exactly.

**Cache lifecycle:**

| Event | Action |
|-------|--------|
| `fm task new` or `fm task load` — after checkout | Cache written |
| `fm task hold` — before switching to baseline | Cache cleared |
| `fm task complete` — before switching to baseline | Cache cleared |
| Current branch ≠ cached branch | Cache ignored (stale) |

The `{repo-hash}` is an FNV-1a hash of the git repository root path, so separate repositories on the same machine never share a cache entry.

### Idempotency

Every `fm` command is idempotent. Running it twice produces the same result as running it once:

- If a WI already exists with the expected title and type → reuse it, do not create a duplicate.
- If the remote branch already exists → do not error; use it.
- If a PR already exists for the branch → do not create a second one; use the existing one.
- If a link already exists on the WI → skip creation, do not duplicate.
- If a state transition is already in the desired state → succeed silently.
- If a stash with the expected name already exists → succeed silently (do not re-stash).

### ID disambiguation

Several commands accept a generic `<id>` argument:

| Format | Resolution |
|--------|------------|
| `76987698` (plain number) | Try as both PR id and WI id. |
| `w-123`, `wi-123`, `w123` | Force WI lookup only. |
| `pr-123`, `p-123` | Force PR lookup only. |
| `feature/123-slug`, `fix/123-slug` | Extract WI id from branch name. |

### Global options

- `--format markdown|json` — output format (default: `markdown`).

---

## 2. Configuration

### `fm init`

```
fm init
  [--path <file>]     output path (default: fm.toml in current directory)
  [--discover]        auto-detect provider from .env and git remote
```

Without `--discover`: writes a commented TOML template to the output path and exits. Refuses to overwrite an existing file.

With `--discover`:
1. Reads `.env` in the current directory for known keys (`ADO_PAT`, `ADO_URL`, `ADO_PROJECT`, `SONAR_URL`, `SONAR_TOKEN`, `GITHUB_TOKEN`, `GITLAB_TOKEN`, etc.).
2. Runs `git remote get-url origin` to detect the provider from the URL pattern:
   - `dev.azure.com` or `visualstudio.com` → ADO
   - `github.com` → GitHub
   - `gitlab.com` → GitLab
3. Reads `.gitmodules` to detect submodule paths.
4. Writes a pre-filled `fm.toml` from the discovered values.

### Config file

Configuration is resolved by merging sources in order of increasing precedence:

1. Built-in defaults
2. Config file (`fm.toml`, `fm.yaml`, or any format supported by the `config` crate)
3. `.env` file (loaded via `dotenvy`)
4. Environment variables (prefix `FM__`, double underscore for nesting)

#### Provider (`[provider]`)

```toml
[provider]
type = "ado"          # "ado" | "github" | "gitlab"

[provider.ado]
url     = "https://dev.azure.com/your-org"
project = "your-project"
pat     = "YOUR_PAT_HERE"
```

#### Workflow defaults (`[fm]`)

```toml
[fm]
merge_strategy = "squash"      # squash | rebase | rebaseMerge | noFastForward
default_target = "main"
submodules     = ["_docs"]     # paths managed transparently by fm commit/push/sync
```

#### SonarQube (`[sonar]`, optional)

```toml
[sonar]
url   = "https://sonar.example.com"
token = "YOUR_SONAR_TOKEN_HERE"
```

#### Environment variable mapping

| Config key | Environment variable |
|---|---|
| `provider.ado.pat` | `FM__PROVIDER__ADO__PAT` |
| `provider.github.token` | `FM__PROVIDER__GITHUB__TOKEN` |
| `fm.default_target` | `FM__FM__DEFAULT_TARGET` |
| `sonar.token` | `FM__SONAR__TOKEN` |

---

## 3. Activity lifecycle (`fm task`)

### `fm task new`

**Goal:** Create a new work item, remote branch, and draft PR in one step, then switch locally to the new branch.

```
fm task new
  --title <title>                   (required)
  [--description <text>]
  [--branch <slug>]                 branch name suffix; defaults to slugified title
  [--type feature|fix]              default: feature  →  User Story / Bug
  [--target <base-branch>]          default: fm.default_target
  [--assigned-to <email>]
  [--tags <tag1;tag2>]
  [--sonar-project <key>]
```

**Steps:**

1. Create WI (`User Story` for `feature`, `Bug` for `fix`) with title, description, tags.
2. If `--sonar-project`: fetch open Sonar issues and append to WI description.
3. Derive branch name: `{type}/{wi-id}-{slug}`.
4. Create remote branch from `--target` via the VCS provider.
5. Create draft PR linked to the WI, targeting `--target` (`workItemRefs` set at creation time).
6. Set WI state → **Active**.
7. `git fetch && git checkout {branch}`.

**Output:**

```markdown
## New Activity Started

| | |
|-|---|
| Work Item | #73240 — login flow implementation |
| Type      | User Story |
| State     | Active |
| Branch    | `feature/73240-login-flow` |
| PR        | #15650 (draft) |
| Target    | `main` |
```

**Errors:**
- If the branch already exists remotely: error "branch already exists — use `fm task load`".

---

### `fm task load`

**Goal:** Resume an existing work item. Repairs missing branch or PR if needed, restores any stashed work, and switches to the activity branch.

```
fm task load <task>
  [--target <base-branch>]
```

`<task>` accepts: WI id, branch name, or any disambiguated format (see §1).

**Steps:**

1. Resolve `<id>` to a WI.
2. **If WI is Closed/Done:** print summary and exit — no branch switch.
3. **If WI is Active or New:**
   a. Scan `git branch -r` for any branch matching `/{wi-id}-`; fall back to deriving the name from WI id and title.
   b. `git fetch && git checkout {branch}`.
   c. Write branch cache (`branch → wi_id`) so non-conventional branch names are recognized by later commands.
   d. Set WI state → **Active** if not already.
   e. Restore stashes if present:
      - Pop `stash-{wi-id}-unstaged` first (normal working-tree changes).
      - Pop `stash-{wi-id}-staged` with `--index` to re-stage those files.

**Errors:**
- Branch not found locally or remotely: error with branch name.
- Stash conflict on restore: conflict message printed; stash left intact for manual resolution.

---

### `fm task show`

**Goal:** Display detailed information about a work item, optionally including comments.

```
fm task show <task>
  [--comments]    show comments for the work item
```

`<task>` accepts: WI id, branch name, or any disambiguated format (see §1). Without `--comments`: shows basic WI information.

**Parallel fetching:** The WI details and comments are fetched in parallel when `--comments` is specified.

**Output (basic):**

```markdown
## Work Item #73240 — login flow implementation

| Field   | Value |
|--------|-------|
| ID      | #73240 |
| Title  | login flow implementation |
| Type   | User Story |
| State  | Active |
| Assigned To | user@email.com |
| Comments | 2 |
```

**Output (with `--comments`):**

```markdown
## Work Item #73240 — login flow implementation

| Field   | Value |
|--------|-------|
| ID      | #73240 |
| Title  | login flow implementation |
| Type   | User Story |
| State  | Active |
| Assigned To | user@email.com |
| Comments | 2 |

### Comments

1. **user@email.com** — 2024-01-15 10:30:
   Working on the authentication service first.

2. **user@email.com** — 2024-01-16 14:00:
   Ready for review.
```

---

### `fm task comment`

**Goal:** Add a comment to the current work item.

```
fm task comment
  --message <text>
```

Derives WI from current branch. Errors if on baseline. Posts a comment via the issue tracker API.

**Output:**

```
Comment added to WI #73240.
```

---

### `fm task list`

**Goal:** List work items filtered by state, type, and assignee.

```
fm task list
  [--mine]                  filter by current user
  [--state <state>]         default: Active
  [--type feature|fix|all]  default: all
  [--max <n>]               default: 20
```

Outputs a Markdown table: `ID | Type | State | Title | Assigned To`.

---

### `fm task hold`

**Goal:** Safely pause the current activity, push committed work, and return to the baseline branch.

```
fm task hold
  [--force]     discard uncommitted changes instead of stashing (destructive)
  [--stay]      stay on the current branch after hold
```

**Steps:**

1. If on **baseline**: print "Already on baseline, nothing to hold." and exit.
2. Check `git status`.
3. **If dirty and `--force`:** `git checkout -- .` (changes discarded).
4. **If dirty (default — auto-stash):** save as two named stashes:
   - If staged changes exist: `git stash push --staged -m "stash-{wi-id}-staged"`
   - If unstaged changes remain: `git stash push -m "stash-{wi-id}-unstaged"`
5. `git push`.
6. Clear branch cache.
7. Switch to baseline (unless `--stay`).

**Output:**

```
Stashed changes as `stash-73240-`.
Moved to baseline `main`
```

---

### `fm task update`

**Goal:** Update the WI linked to the current activity.

```
fm task update
  [--title <title>]
  [--state <state>]
  [--description <text>]
  [--assigned-to <email>]
  [--tags <tag1;tag2>]
```

Derives WI from current branch. Errors if on baseline. Applies requested fields via the issue tracker PATCH API.

---

### `fm task sync`

**Goal:** Update the current activity branch with commits from the baseline (merge or rebase).

```
fm task sync
  [--rebase]    use rebase instead of merge (default: merge)
  [--check]     dry-run: show commits behind/ahead without modifying
```

**Steps:**

1. Error if on baseline.
2. `git fetch origin`.
3. **`--check`:** compare HEAD against `origin/{target}`, print divergence summary, exit.
4. **Merge (default):** `git merge origin/{target}`.
5. **`--rebase`:** `git rebase origin/{target}`.
6. On conflict: print git output and recovery instructions, exit non-zero.
7. On clean: `git push`.

**Recovery instructions (on conflict):**

```
Conflicts detected. Resolve them with git directly:
  git status
  # edit files to resolve conflicts
  git add <resolved-files>
  git rebase --continue   ← if --rebase
  git merge --continue    ← if merge
  fm push
```

---

### `fm task complete`

**Goal:** Verify the activity is fully done (WI closed, PR merged or abandoned) and return to an updated baseline.

```
fm task complete
```

**Steps:**

1. Error if on baseline.
2. Fetch WI state and linked PR state.
3. Error if PR is still active/draft — merge or publish it first.
4. Switch to `default_target`, `git pull`.
5. Clear branch cache.

---

## 4. Pull Requests (`fm pr`)

### `fm pr show [<id>]`

**Goal:** Assemble a `context.md` document suitable as input to an AI review agent.

```
fm pr show [<id>]
  [--out <file>]               write to file instead of stdout
  [--include-project-context]  inject README / AGENTS.md / CONTRIBUTING.md
```

No `<id>` → uses current branch PR. `<id>` resolved via disambiguation rules (§1).

**What it fetches:**

- PR metadata (title, author, branches, status, description) — `GET /pullRequests/{id}`
- All threads with status and comments — `GET /pullRequests/{id}/threads`
- Changed file list — `GET /pullRequests/{id}/iterations/{n}/changes`
- Project context files from repo root — local filesystem (only when `--include-project-context`)

**Output format (`context.md`):**

```markdown
# PR #15650 — login flow implementation

**Author:** marco.rossi
**Target branch:** main
**Source branch:** feature/73240-login-flow
**Created:** 2024-06-01T10:22:00Z
**Status:** active

---

## Description

Implements the login flow using OAuth2.

### Open Points

- [ ] Handle token refresh edge case
- [ ] Add integration tests

---

## Threads
<!-- total: 2  active: 1  resolved: 1 -->

### Thread 42 · active
**File:** `src/auth/login.ts` line 88
**Author:** elena.v · 2024-06-02T09:10:00Z

Max retries are hardcoded as `3`. Should come from config.

---

## Changed Files

| File | Change |
|---|---|
| src/auth/login.ts | edit |
| tests/auth/login.test.ts | add |

---

## Project Context

### README.md
> ...
```

Threads are sorted active-first, then resolved. Resolved threads are included for context. Open points are extracted from the PR description checklist syntax (`- [ ]` / `- [x]`). Project context section only appears when `--include-project-context` is passed.

---

### `fm pr thread list`

**Goal:** List comment threads on a PR.

```
fm pr thread list [<id>]
  [--status active|resolved|all]   default: active
```

No `<id>` → uses current branch PR.

**Output:**

```
ID     STATUS     FILE                                LINE   AUTHOR          PREVIEW
-----------------------------------------------------------------------------------------------
42     active     src/auth/login.ts                   88     elena.v         Max retries are hardco…
31     resolved   src/auth/oauth.ts                   14     elena.v         Missing null check on…
```

---

### `fm pr thread reply`

**Goal:** Post a reply to an existing thread.

```
fm pr thread reply <thread-id> <message>
  [--pr <id>]     PR id (optional, uses current context if omitted)
  [--resolve]     resolve the thread after posting the reply
```

`<message>` accepts `-` to read from stdin.

**Output:**

```
Reply posted to thread 42.
Thread 42 resolved.          ← only when --resolve
```

---

### `fm pr thread resolve`

**Goal:** Resolve one or more threads, with an optional closing comment.

```
fm pr thread resolve <thread-id> [<thread-id>...]
  [--pr <id>]          PR id (optional, uses current context if omitted)
  [--comment <text>]   optional comment posted before resolving
```

Sets thread status to `fixed` in ADO. When `--comment` is given the comment is posted first.

**Output:**

```
Thread 42 resolved.
Thread 47 resolved.
```

---

### `fm pr feedback validate`

**Goal:** Validate an agent-produced review file against the schema and cross-reference it against live ADO data. **Read-only. No writes.**

```
fm pr feedback validate
  --file <path>          path to review.yaml or review.md
  [--pr <id>]            PR id (optional, uses current context if omitted)
  [--format yaml|md]     explicit format; auto-detected from extension if omitted
```

**Exit codes:**

| Code | Meaning |
|------|---------|
| `0` | Valid — no errors |
| `1` | Hard errors — apply must not proceed |
| `2` | Warnings only — apply can proceed with `--force` |

**Checks performed:**

- `summary` length ≥ 10 characters
- `recommendation` is one of `approve`, `request_changes`, `needs_discussion`
- Each `threads[]` entry: `action` is `resolve` or `reply`; thread id exists in the PR; warns if thread already resolved
- Each `new_threads[]` entry: `severity` is one of `critical`, `major`, `minor`, `positive`; warns if file is outside the PR diff
- Each `open_points[]` entry: `status` is one of `addressed`, `not_addressed`, `partially_addressed`; `ref` must match an open point from the PR description

**Output:**

```
Validating review.yaml against PR #15650…

Schema
  ✅ Valid YAML
  ✅ Required fields present (summary, recommendation)
  ✅ recommendation value: request_changes

Threads
  ✅ Thread 42 exists · status: active → action: resolve ✓
  ⚠️  Thread 31 exists but is already resolved → action will be skipped

New Threads
  ✅ src/auth/login.ts exists in repo
  ⚠️  src/auth/config.ts is outside the PR diff — will still be posted

Open Points
  ✅ "Handle token refresh edge case" matches context open point
  ❌ "Add load tests" not found in PR open points

Result: 1 error, 2 warnings — fix errors before applying.
```

---

### `fm pr feedback apply`

**Goal:** Execute all actions from a validated review file against the PR. **This is the only command that writes to ADO.**

```
fm pr feedback apply
  --file <path>          path to review.yaml or review.md
  [--pr <id>]            PR id (optional, uses current context if omitted)
  [--format yaml|md]     explicit format; auto-detected if omitted
  [--dry-run]            print every action that would be taken, no writes
  [--force]              apply despite validation warnings (errors still block)
```

Runs `validate` internally before any write. Aborts on validation errors; aborts on warnings unless `--force` is passed.

**Execution order:**

1. Post overall summary as a new PR thread (from `summary` field)
2. Reply to existing threads (`threads[].action = reply`)
3. Resolve existing threads (`threads[].action = resolve`)
4. Post new file-level threads (`new_threads`)
5. Post open points summary as a reply on the summary thread

**Exit codes:**

| Code | Meaning |
|------|---------|
| `0` | All actions applied successfully |
| `1` | Validation failed — nothing written |
| `2` | Partial failure — some ADO API calls failed |

**Output:**

```
Applying review to PR #15650…

[1/5] Posting summary comment…              ✅ Thread 89 created
[2/5] Replying to thread 47…                ✅ Reply posted
[3/5] Resolving thread 42…                  ✅ Resolved
[4/5] Posting new thread on src/auth/login.ts:88…   ✅ Thread 90 created
[5/5] Posting new thread on src/auth/oauth.ts:34…   ✅ Thread 91 created
      Open points summary…                  ✅ Posted as reply on thread 89

Done. 5 actions applied, 0 failed.
```

---

### Review file formats

`fm pr feedback validate` and `fm pr feedback apply` accept two input formats.

#### `review.yaml` (preferred)

```yaml
summary: |
  Two of three open points addressed. One major issue on token refresh.

recommendation: request_changes

threads:
  - id: 42
    action: resolve
    comment: Config value now read from env. Hardcoding removed.

  - id: 47
    action: reply
    comment: TODO comment is not acceptable for merge. Open a tracked work item.

new_threads:
  - file: src/auth/oauth.ts
    line: 34
    severity: major
    comment: Token not scoped per-user. Two concurrent timeouts could collide.

open_points:
  - ref: "Handle token refresh edge case"
    status: not_addressed
    comment: No changes relating to token refresh found in the diff.
```

#### `review.md` (alternative)

Supported via `--format md`. The CLI parses ` ```action:<type> ``` ` fenced blocks embedded in the prose. All other Markdown content is used as the summary comment.

```markdown
# Review — PR #15650

Two of three open points addressed.

```action:thread
id: 42
action: resolve
comment: Config value now read from env.
```

```action:new_thread
file: src/auth/oauth.ts
line: 34
severity: major
comment: Token not scoped per-user.
```

**Recommendation:** request_changes
```

---

### `fm pr feedback structure`

**Goal:** Print a plain-text description of the review file format. No network calls.

```
fm pr feedback structure
```

Outputs a human-readable reference covering all fields, allowed values, and the `review.md` alternative format. Useful as a prompt preamble when instructing an AI agent to produce a review file.

---

### `fm pr feedback schema`

**Goal:** Print the JSON Schema for `review.yaml`. No network calls.

```
fm pr feedback schema
```

Outputs the full draft-07 JSON Schema. Can be piped directly into a validator or embedded in an agent prompt:

```bash
fm pr feedback schema > review-schema.json
fm pr feedback schema | pbcopy   # macOS clipboard
```

---

### `fm pr update`

**Goal:** Update the PR linked to the current activity.

```
fm pr update
  [--title <title>]
  [--description <text>]
  [--publish]                    remove draft status
  [--status active|abandoned|completed]
  [--add-reviewer <email>]       repeatable
```

Derives PR from current branch. Prints `PR #{id} updated.` on success.

---

### `fm pr merge`

**Goal:** Complete the PR linked to the current activity using the configured merge strategy.

```
fm pr merge
  [--strategy squash|rebase|rebaseMerge|noFastForward]
  [--delete-source-branch]
  [--bypass-policy]
```

**Steps:**

1. Error if on baseline.
2. Error if PR is still a draft — publish it first.
3. Apply merge strategy (default: `fm.merge_strategy`).
4. Complete the PR via the VCS provider.
5. Set WI state → **Closed**.

**Merge strategies:**

| Strategy | Description |
|----------|-------------|
| `squash` | All commits squashed into one (default) |
| `rebase` | Rebase source commits onto target, no merge commit |
| `rebaseMerge` | Rebase + merge commit |
| `noFastForward` | Standard merge commit, always created |

---

### `fm pr review <id>`

**Goal:** Temporarily switch to another PR's branch to review it, safely pausing the current activity first.

```
fm pr review <id>
```

**Steps:**

1. If in **Activity** context with dirty working tree: auto-stash (`stash-{wi-id}-staged` / `stash-{wi-id}-unstaged`) and push.
2. If clean: push and switch.
3. Resolve `<id>` to a PR.
4. `git fetch && git checkout {pr-branch}`.

**Resuming after review:**

```bash
fm task load <original-wi-id>    # restores stash and switches back
```

---

## 5. Todos (`fm todo`)

Child Tasks linked to the current User Story.

### Todo resolution (`<ref>`)

1. **Exact numeric ID** — direct lookup, validated as child of current WI.
2. **Title substring** (case-insensitive) — searches within current WI's children.
   - One match: use it.
   - Multiple matches: list them and exit non-zero.
   - No match: error with suggestion to run `fm todo show`.

---

### `fm todo show`

```
fm todo show
  [--all]      include closed/done items
  [--detail]   show description under each item
```

Errors if on baseline. Groups by state: Active first, then New, then Closed (only with `--all`).

**Output:**

```markdown
## Todos — #73240: login flow implementation

  ●  #73243  add tests                    Active
  ○  #73241  implement login UI
  ○  #73244  update documentation

  ─────────────────────────────────────────
  0 done · 1 active · 2 open · 3 total
```

Legend: `●` Active · `○` New · `✓` Closed

---

### `fm todo new`

```
fm todo new
  --title <title>
  [--description <text>]
  [--assigned-to <email>]
  [--pick]                 set Active immediately
```

Creates a Task linked as a child of the current WI.

---

### `fm todo pick <ref>`

Sets the referenced todo to **Active**.

---

### `fm todo complete <ref>`

Sets the referenced todo to **Closed**.

---

### `fm todo reopen <ref>`

Sets the referenced todo back to **New**.

---

### `fm todo update <ref>`

```
fm todo update <ref>
  [--title <title>]
  [--description <text>]
  [--assigned-to <email>]
  [--state <state>]
```

---

### `fm todo next`

```
fm todo next
  [--pick]    set Active immediately
```

Shows the first New todo (by creation order). Suggests `fm task complete` if all todos are closed.

---

## 6. Pipelines (`fm pipeline`)

### `fm pipeline run`

```
fm pipeline run
  [--id <pipeline-id>]
```

Triggers a CI pipeline run on the current branch. If `--id` is omitted: prints the pipeline list and exits non-zero — caller must re-run with `--id`.

---

### `fm pipeline status`

```
fm pipeline status
  [--run-id <id>]
  [--watch]         poll every 30s until completed
```

Shows the latest CI run status for the current branch. `--run-id` to inspect a specific run.

---

## 7. Source control

### `fm commit`

```
fm commit
  [--message <msg>]        auto-generated from WI context if omitted
  [--all]                  stage all tracked changes before committing
  [--amend]                amend the last commit
  [--docs-message <msg>]   override submodule commit message
  [--no-docs]              skip submodule handling
```

**Submodule detection (applied unless `--no-docs`):**

| Submodule state | Action |
|-----------------|--------|
| Clean, up to date | Skip |
| Uncommitted changes | `git add -A` + commit in submodule, then push |
| Unpushed commits only | Push submodule only |
| Both | Commit then push submodule |

After any submodule push, the parent repo's pointer is staged and included in the main commit.

**Auto-generated message** (when `--message` omitted, Activity context):
```
[#{wi-id}] {wi-title}: <summary>
```

---

### `fm push`

```
fm push
  [--force]     use --force-with-lease
  [--no-docs]   skip submodule push check
```

Checks submodule for unpushed commits (same detection as `fm commit`), pushes submodule first if needed, then pushes the current branch.

---

### `fm sync`

```
fm sync
  [--message <msg>]
  [--docs-message <msg>]
```

Shorthand for `fm commit --all` + `fm push`. If nothing to commit: prints "Nothing to commit. Working tree clean."

---

## 8. Quality

### `fm sonar list`

```
fm sonar list
  [--search <pattern>]    Wildcard search (*, ?)
  [--favorites]         Only favorited projects
```

Lists SonarQube projects. Use `--search` for wildcard filtering.

### `fm sonar issues`

```
fm sonar issues
  [--project <key>]      Project key (uses first from config if omitted)
  [--all]               Fetch all configured projects in parallel
  [--severity <levels>]  e.g. MAJOR,CRITICAL,BLOCKER
  [--max <n>]           default: 20
```

Lists open SonarQube issues. Requires `[sonar]` config. Without `--project`, uses the first project from `[sonar].projects`.

To fetch issues for all configured projects:

```
fm sonar issues --all
```

This runs parallel requests for each project in `[sonar].projects`.

---

## 9. Context

### `fm context`

```
fm context
  [--only-task]
  [--only-pr]
  [--only-git]
  [--only-pipeline]
  [--task-comments]    show comments for the current work item
```

**Baseline branch:** prints branch name and last commits.

**Activity branch:** fetches and displays:
- WI details (id, title, state, assigned to, comments count)
- PR details (state, draft, target branch)
- Git status (ahead/behind, local changes)
- Latest CI pipeline run (unless `--only-task` or `--only-pr` or `--only-git`)
- Comments (if `--task-comments`)

**Parallel fetching:** In Activity context, the WI, PR, Git status, and pipeline data are fetched in parallel when possible.

**Output (activity):**

```markdown
## Context — `feature/73240-login-flow`

### Work Item
| ID       | #73240 |
| Title    | login flow implementation |
| State    | Active |
| Comments | 2 |

### Pull Request
| PR     | #15650 |
| State  | draft |
| Target | `main` |

### Git
| Ahead  | 3 commits |
| Behind | 0 commits |
| Local  | clean |
```

**Output (with `--task-comments`):**

```markdown
## Context — `feature/73240-login-flow`

### Work Item
| ID       | #73240 |
| Title    | login flow implementation |
| State    | Active |
| Comments | 2 |

### Comments

1. **user@email.com** — 2024-01-15 10:30:
   Working on the authentication service first.

2. **user@email.com** — 2024-01-16 14:00:
   Ready for review.

### Pull Request
...
```

---

## 10. Diagnostics

### `fm doctor`

```
fm doctor
  [--fix]    attempt to repair broken invariants
```

Checks and reports:
- Git installed and inside a git repository
- VCS provider reachable (calls `get_repository`)
- SonarQube config present and buildable
- Configured submodule paths exist

With `--fix` and in Activity context: sets WI state to Active and attempts to repair missing artifact links.

---

## 11. Typical workflows

### Start a new feature

```bash
fm task new --title "login flow implementation" --branch "login-flow" --sonar-project my-project
# → WI created, branch created, draft PR created, WI Active, local branch checked out
```

### Daily loop

```bash
fm context                    # understand where you are

# ... write code ...

fm sync --message "wip: auth service"   # commit + push, submodule handled transparently
fm task sync --check                    # check drift from main
fm task sync --rebase                   # sync with main

fm task hold                            # push and switch to baseline
fm task load 73240                      # next morning: restore context
```

### Parallel review

```bash
fm pr review 15700            # stash current work, switch to review branch
fm task load 73240            # back to your work, stash restored
```

### Completing work

```bash
fm pr update --publish        # remove draft
fm pipeline run --id 614      # trigger CI
fm pipeline status --watch    # watch CI
fm pr merge                   # merge (uses fm.merge_strategy)
fm task complete              # switch to main and pull
```

---

## 12. Scope

`fm` covers the ~80% of daily workflow that is repetitive and automatable.

| Scenario | Command |
|----------|---------|
| Start new work | `fm task new` |
| Resume existing work | `fm task load` |
| Understand current state | `fm context` |
| Save and pause work | `fm task hold` / `fm sync` |
| Commit with submodule handling | `fm commit` / `fm sync` |
| Manage todos | `fm todo *` |
| Keep branch up to date | `fm task sync [--rebase]` |
| Publish PR for review | `fm pr update --publish` |
| Assemble AI review context | `fm pr show [--out context.md] [--include-project-context]` |
| List / reply / resolve threads | `fm pr thread list / reply / resolve` |
| Validate AI review file | `fm pr feedback validate --file review.yaml` |
| Apply AI review to PR | `fm pr feedback apply --file review.yaml` |
| Merge PR | `fm pr merge` |
| Trigger and watch CI | `fm pipeline run` / `fm pipeline status` |
| Finalise and return to baseline | `fm task complete` |

The remaining ~20% — conflict resolution, cherry-picks, interactive rebase, bulk WI queries, policy bypasses — is handled directly with `git`, the provider's CLI, or the web UI. `fm` never hides errors from these underlying layers.
