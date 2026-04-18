# `fm.cs` — Flow Manager: Porcelain Commands Proposal

> **Status:** WIP / Proposal  
> **Scope:** Specification only — no implementation yet  
> **Wraps:** `ado.cs` (ADO REST API), `sonar.cs` (SonarQube), `git` (local/remote)

---

## 1. Concepts

### Context model

At any point the developer is in one of two contexts:

| Context | Description |
|---------|-------------|
| **Baseline** | On a protected/shared branch (`main`, `develop`). No active work item. |
| **Activity** | On a `feature/*` or `fix/*` branch. Linked to a WI, a remote branch, and a draft PR. |

The **current branch is the source of truth** for the active context. `fm` reads the branch name to derive the work item ID and activity type.

### Branch naming

```
feature/{wi-id}-{short-slug}    ← User Story (evolution/feature)
fix/{wi-id}-{short-slug}        ← Bug
```

Examples:
```
feature/73235-pipeline-list-workflow-script
fix/73100-login-redirect-loop
```

- `{wi-id}` is the ADO work item ID.
- `{short-slug}` is kebab-case, max ~40 chars, derived from the WI title or supplied via `--branch`.

### Activity invariants

When in **Activity** context, the following must always be true:

1. A WI exists and is **Active**.
2. A remote branch exists (same name as local).
3. A **draft or active PR** exists, linked to the WI and targeting the baseline branch.
4. The **ADO work item links** record both the branch and the PR as linked artifacts on the WI:
   - A `Branch` artifact link: `vstfs:///Git/Ref/{repo-id}/{branch-name}`
   - A `Pull Request` artifact link: `vstfs:///Git/PullRequestId/{repo-id}/{pr-id}`

`fm work new` and `fm work load` both enforce and repair these invariants. Every command that operates in Activity context **validates all four invariants** before proceeding — and attempts to repair missing links silently before raising an error.

The branch name is a shortcut for human readability; the canonical link is the ADO artifact relation. When branch name and WI link diverge, the artifact link wins.

### Idempotency

**Every `fm` command is idempotent.** Running it twice must produce the same result as running it once. Concretely:

- If a WI already exists with the expected title and type → reuse it, do not create a duplicate.
- If the remote branch already exists → do not error; use it.
- If a PR already exists for the branch → do not create a second one; use and update the existing one.
- If a link already exists on the WI → skip the link creation, do not duplicate.
- If a state transition is already in the desired state → succeed silently.
- If a stash with the expected name already exists → succeed silently (do not re-stash).

Idempotency enables safe retry after partial failure, resumption mid-flow, and direct invocation by AI agents without side-effect risk.

### ID disambiguation

Several commands accept a generic `<id>` argument. Resolution rules:

| Format | Resolution |
|--------|-----------|
| `76987698` (plain number) | Try as both PR id and WI id. Error if both match. |
| `w-123`, `wi-123`, `w123` | Force WI lookup only. |
| `pr-123`, `p-123` | Force PR lookup only. |
| `feature/123-slug`, `fix/123-slug` | Extract WI id from branch name, look up WI + PR. |

### Environment variables

`fm` is fully non-interactive and configurable via environment variables. All flags that change default behaviour can be set persistently in `.env`.

| Variable | Default | Description |
|----------|---------|-------------|
| `ADO_URL` | — | Azure DevOps organisation URL (required) |
| `ADO_PAT` | — | Personal Access Token (required) |
| `ADO_PROJECT` | — | ADO project name (required) |
| `SONAR_URL` | — | SonarQube base URL |
| `SONAR_TOKEN` | — | SonarQube token |
| `FM_MERGE_STRATEGY` | `squash` | Default PR merge strategy: `squash`, `rebase`, `rebaseMerge`, `noFastForward` |
| `FM_DEFAULT_TARGET` | `main` | Default target branch for new PRs and branches |
| `FM_DEFAULT_WI_TYPE` | `User Story` | Default work item type for `fm work new` |
| `FM_DOCS_SUBMODULE` | `_docs` | Relative path of the docs submodule (empty = disabled) |
| `FM_FORMAT` | `markdown` | Default output format: `markdown`, `json` |

---

## 2. Commands

---

### `fm work new`

**Goal:** Create a new User Story (or Bug), a branch, and a draft PR in one step. Move to the new branch.

#### Synopsis

```
fm work new
  --title <title>                   (required)
  [--description <text>]
  [--branch <slug>]                 short suffix for branch name; defaults to slugified title
  [--type feature|fix]              default: feature  →  User Story / Bug
  [--target <base-branch>]          default: main
  [--assigned-to <email>]
  [--tags <tag1;tag2>]
  [--sonar-project <key>]           if provided, fetches open Sonar issues and appends to description
  [--format markdown|json]          default: markdown
```

#### Steps

1. Create ADO WI (`User Story` for `feature`, `Bug` for `fix`) with title, description, tags.
2. If `--sonar-project` is given, fetch open issues and append as an HTML list to the WI description.
3. Derive branch name: `{type}/{wi-id}-{slug}`.
4. Create remote branch from HEAD of `--target` via ADO Git Refs API.
5. Create draft PR linked to WI, targeting `--target`.
6. Set WI state → **Active**.
7. `git fetch && git checkout {branch}` to move locally.

#### Output

```markdown
## New Activity Started

| | |
|-|---|
| Work Item | #73240 — login flow implementation |
| Type      | User Story |
| State     | Active |
| Branch    | `feature/73240-login-flow` |
| PR        | #15650 (draft) |
| Mergeable | — (no commits yet) |
| Target    | `main` |
```

#### Variants / errors

- If the branch already exists remotely: error "branch already exists — use `fm work load`".
- If `--type fix` but WI type is forced to Bug; `--title` is mandatory.

---

### `fm work load`

**Goal:** Resume an existing work item. Repairs missing branch or PR if needed, restores any stashed work, and switches to the activity branch.

#### Synopsis

```
fm work load <id>
  [--target <base-branch>]    default: main (used if branch/PR need to be created)
  [--format markdown|json]
```

`<id>` accepts: WI id, branch name, or any disambiguated format (see §1).

#### Steps

1. Resolve `<id>` to a WI. If not found, error with suggestions.
2. **If WI state is Closed/Done:** print summary (id, title, state, merged PR if any) and exit — no branch switch.
3. **If WI is Active or New:**
   a. Derive expected branch name from WI id and title slug.
   b. Check if remote branch exists → create from `--target` if missing.
   c. Check if a draft or active PR exists for the branch → create draft if missing.
   d. Ensure WI state is **Active** (set if New).
   e. `git fetch && git checkout {branch}`.
   f. If a stash named `stash-{wi-id}-*` exists, restore it (`git stash pop`).

#### Output

Same structure as `fm work new`. Adds a `Stash restored` line if applicable.

#### Variants / errors

- WI not found: list recent active WIs as suggestions.
- Multiple PRs for the same branch: print all matches with id, title, state and exit non-zero — caller must specify with `pr-<id>`.
- Stash conflict on restore: show conflict message; stash left intact for manual resolution.

---

### `fm context`

**Goal:** Snapshot of the current activity. Entry point for every new work session.

#### Synopsis

```
fm context
  [--only-wi]        show only work item details
  [--only-pr]        show only PR details
  [--only-git]       show only git status
  [--only-pipeline]  show only latest CI run for this branch
  [--format markdown|json]
```

#### Steps

1. Read current branch name.
2. **Baseline branch:** print branch name and last 5 commits. No further lookups.
3. **Activity branch:** extract WI id from branch name, then:
   a. Fetch WI details (id, title, state, assigned to, tags).
   b. Find PR for this branch (state, draft, mergeable, reviewer count).
   c. Run `git status` and `git log --oneline origin/{target}..HEAD` (ahead/behind).
   d. Fetch latest CI pipeline run for this branch (state, result, url).

#### Output (activity branch)

```markdown
## Context — `feature/73240-login-flow`

### Work Item
| | |
|-|---|
| ID    | #73240 |
| Title | login flow implementation |
| State | Active |
| Assigned | marco.denittis@synesthesia.it |

### Pull Request
| | |
|-|---|
| PR       | #15650 |
| State    | draft |
| Mergeable| not yet (no reviewers) |
| Target   | `main` |

### Git
| | |
|-|---|
| Ahead  | 3 commits |
| Behind | 0 commits |
| Local  | clean |

### CI
| | |
|-|---|
| Pipeline | pipeline-be-ci (#614) |
| Last run | #59530 — inProgress |
```

#### Output (baseline branch)

```markdown
## Context — `main` (baseline)

Last commits:
- 429d403 chore: update _docs submodule
- b1f3c9b ado.cs: add pipeline list, workflow start...
- ad782bd tools for work with azure devops
```

---

### `fm task hold`

**Goal:** Safely pause the current activity, push committed work, and return to the baseline branch.

#### Synopsis

```
fm task hold
  [--stash]     stash uncommitted changes before holding
  [--force]     discard uncommitted changes (destructive)
  [--stay]      stay on the current branch after hold (don't switch to baseline)
  [--format markdown|json]
```

#### Steps

1. If in **baseline** branch: print "Already on baseline, nothing to hold." and exit.
2. Check `git status`.
3. **If working tree is clean:** `git push`, switch to baseline (unless `--stay`).
4. **If working tree is dirty and no flag:** show `git status` output and message:
   > "Uncommitted changes present. Use `--stash` to save them or `--force` to discard."
   Exit without doing anything.
5. **`--stash`:** run `git stash push -m "stash-{wi-id}-{slug}"`, then push, then switch to baseline.
6. **`--force`:** run `git checkout -- .` (discard), then push, then switch to baseline.

#### Output

```markdown
## Task Hold

| | |
|-|---|
| Branch pushed | `feature/73240-login-flow` |
| Stash         | `stash-73240-login-flow` saved |
| Now on        | `main` |
```

---

### `fm pr show`

**Goal:** Display PR details for the current or a specified context.

#### Synopsis

```
fm pr show [<id>]
  [--format markdown|json]
```

No `<id>` → uses current activity branch PR.

#### Steps

1. Resolve `<id>` using disambiguation rules (§1), or derive from current branch.
2. If `<id>` matches both a PR and a WI of this project: show error with both items' id, title, state.
3. If resolved to a WI: find the linked PR.
4. Fetch PR details: title, state, draft, source/target branches, created by, reviewers, merge status, linked WIs, comments count.

#### Output

```markdown
## PR #15650 — login flow implementation

| Field      | Value |
|------------|-------|
| State      | draft |
| Branches   | `feature/73240-login-flow` → `main` |
| Created By | Marco De Nittis |
| Created    | 2026-04-17 |
| Reviewers  | 0 assigned |
| Linked WI  | #73240 — login flow implementation (Active) |
| Comments   | 2 |
| Mergeable  | not yet |
```

---

### `fm pr update`

**Goal:** Update the PR linked to the current activity context.

#### Synopsis

```
fm pr update
  [--title <title>]
  [--description <text>]
  [--publish]            remove draft status
  [--status active|abandoned|completed]
  [--add-reviewer <email>]
  [--format markdown|json]
```

#### Steps

1. Derive PR from current branch. Error if on baseline.
2. Apply requested changes via ADO PR PATCH.
3. If `--publish`: set `isDraft=false`.
4. Print updated PR summary.

---

### `fm pr merge`

**Goal:** Complete (merge) the PR linked to the current activity context, applying the configured merge strategy.

#### Synopsis

```
fm pr merge
  [--strategy squash|rebase|rebaseMerge|noFastForward]
                        override FM_MERGE_STRATEGY for this call
  [--delete-source-branch]
                        delete the remote source branch after merge (default: keep)
  [--bypass-policy]     bypass branch policies (requires elevated PAT permissions)
  [--format markdown|json]
```

#### Merge strategies

| Strategy | ADO value | Description |
|----------|-----------|-------------|
| `squash` | `squash` | All commits squashed into one on target **(default via `FM_MERGE_STRATEGY`)** |
| `rebase` | `rebase` | Rebase source commits onto target, no merge commit |
| `rebaseMerge` | `rebaseMerge` | Rebase + merge commit |
| `noFastForward` | `noFastForward` | Standard merge commit, always created |

Default is `squash`. Override per call with `--strategy` or persistently with `FM_MERGE_STRATEGY`.

#### Steps

1. Error if on baseline (no active PR context).
2. Fetch PR state: error if draft — caller must publish first (`fm pr update --publish`).
3. Error if PR is not mergeable (policy failures, conflicts) — print merge status details and exit non-zero.
4. PATCH PR with `{"status": "completed", "completionOptions": {"mergeStrategy": "<strategy>", "deleteSourceBranch": <bool>}}`.
5. Set WI state → **Closed**.
6. Print merge summary.

#### Output

```markdown
## PR Merged — #15650

| | |
|-|---|
| Strategy  | squash |
| PR        | #15650 — completed |
| WI        | #73240 — Closed |
| Merged to | `main` |
| Commit    | a3f91bc |

  Run `fm task complete` to switch to main and pull.
```

#### Errors (non-interactive, exit non-zero)

```markdown
## Error — PR Not Mergeable

  PR #15650 cannot be merged.

  Policy failures:
  - Minimum 1 reviewer required (0 approved)
  - Build pipeline-be-ci: failed (run #59530)

  Resolve the above, then re-run `fm pr merge`.
```

---

### `fm task update`

**Goal:** Update the WI linked to the current activity context.

#### Synopsis

```
fm task update
  [--title <title>]
  [--state <state>]          Active, Resolved, Closed, etc.
  [--description <text>]
  [--assigned-to <email>]
  [--tags <tag1;tag2>]
  [--format markdown|json]
```

#### Steps

1. Derive WI from current branch. Error if on baseline.
2. Apply requested fields via ADO WI PATCH.
3. Print updated WI summary.

---

### `fm task complete`

**Goal:** Verify the activity is fully done (WI closed, PR merged or abandoned) and return to an updated baseline.

#### Synopsis

```
fm task complete
  [--format markdown|json]
```

#### Steps

1. If on **baseline**: error "Already on baseline — nothing to complete."
2. Extract WI id from branch, fetch WI state and linked PR state.
3. Check PR state:
   - **merged**: proceed.
   - **abandoned**: proceed (treat as done — WI was likely closed manually).
   - **active / draft**: error with status summary — caller must merge or publish the PR first.
4. Switch to `FM_DEFAULT_TARGET`, `git pull`.
5. Print completion summary.

#### Output

```markdown
## Activity Complete

| | |
|-|---|
| WI     | #73240 — Closed |
| PR     | #15650 — merged |
| Now on | `main` (up to date) |
```

---

### `fm task sync`

**Goal:** Update the current activity branch with commits from the baseline branch (merge or rebase). Conflicts are left to git — `fm` surfaces them and exits; the developer resolves them directly with standard git commands.

#### Synopsis

```
fm task sync
  [--rebase]          use rebase instead of merge (default: merge)
  [--check]           dry-run: show commits behind/ahead without modifying anything
  [--format markdown|json]
```

#### Steps

1. Error if on baseline (nothing to sync from).
2. `git fetch origin`.
3. **`--check` mode:** compare `HEAD` against `origin/{target}`, print divergence summary, exit zero.
4. **Merge mode (default):** `git merge origin/{FM_DEFAULT_TARGET}`.
5. **Rebase mode (`--rebase`):** `git rebase origin/{FM_DEFAULT_TARGET}`.
6. If git exits non-zero (conflicts): print git's conflict output verbatim, print recovery instructions, exit non-zero — **no automatic resolution**.
7. If clean: push the updated branch (`git push`).

#### Recovery instructions (printed on conflict)

```
Conflicts detected. Resolve them with git directly:

  git status                        ← see conflicting files
  # edit files to resolve conflicts
  git add <resolved-files>
  git rebase --continue             ← if --rebase was used
  git merge --continue              ← if merge was used
  fm push                           ← push once resolved
```

#### Output — clean sync

```markdown
## Task Sync — `feature/73240-login-flow`

  Strategy   merge
  From       origin/main  (3 commits behind)
  Result     clean merge  →  pushed

  Commits merged:
  - a1b2c3d  fix: typo in auth handler
  - 9f8e7d6  chore: update lock file
  - 3c2b1a0  feat: add refresh token endpoint
```

#### Output — `--check` mode

```markdown
## Task Sync Check — `feature/73240-login-flow`

  Branch is 3 commits behind origin/main, 2 commits ahead.

  Behind (not yet in branch):
  - a1b2c3d  fix: typo in auth handler
  - 9f8e7d6  chore: update lock file
  - 3c2b1a0  feat: add refresh token endpoint

  Ahead (not yet merged):
  - 7b4d93a  add auth service
  - e1f2g3h  add unit tests

  Run `fm task sync` to merge, or `fm task sync --rebase` to rebase.
```

#### Output — conflict (exit non-zero)

```markdown
## Task Sync — CONFLICT

  Strategy   rebase
  From       origin/main

  Conflicting files:
  - src/auth/AuthService.cs
  - src/auth/AuthService.Tests.cs

  Resolve conflicts manually, then run `fm push`.
```

---

### `fm pr review`

**Goal:** Temporarily switch to another PR's branch to review it, safely pausing the current activity first.

#### Synopsis

```
fm pr review <id>
  [--format markdown|json]
```

`<id>` accepts PR id, WI id, or branch name (see §1).

#### Steps

1. If currently in **Activity** context with dirty working tree:
   - Auto-stash as `stash-{wi-id}-{slug}`.
   - Push committed work.
   - Print "Activity held with stash."
2. If in **Activity** context but clean: push and switch.
3. Resolve `<id>` to a PR.
4. Validate PR belongs to this project. If not: error "PR not in this project."
5. `git fetch && git checkout {pr-branch}`.
6. Print PR summary + `fm context` output for the PR branch.

#### Resuming after review

```bash
fm work load <original-wi-id>    # restores stash and switches back
```

---

### `fm pipeline run`

**Goal:** Trigger a CI pipeline for the current branch.

#### Synopsis

```
fm pipeline run
  [--id <pipeline-id>]    if omitted, auto-detect from branch type (be-ci, fe-chat-ci, etc.)
  [--format markdown|json]
```

#### Steps

1. If on baseline: use the baseline branch as target.
2. If on activity: use the current branch.
3. If `--id` not provided: error — print the pipeline list (id, name) and exit non-zero. Caller must re-run with `--id`.
4. Trigger run, return run id and initial status.

---

### `fm pipeline status`

**Goal:** Show the latest CI run status for the current context branch.

#### Synopsis

```
fm pipeline status
  [--run-id <id>]     if omitted, shows most recent run for current branch
  [--watch]           poll every 30s until completed
  [--format markdown|json]
```

---

### `fm work list`

**Goal:** List active work items, optionally filtered.

#### Synopsis

```
fm work list
  [--mine]             filter by current user (default: all)
  [--state <state>]    default: Active
  [--type feature|fix|all]
  [--max <n>]          default: 20
  [--format markdown|json]
```

#### Steps

Runs WIQL query scoped to the ADO project with the given filters. Output is a table: ID | Type | State | Title | Assigned To | Branch.

---

### `fm sonar`

**Goal:** Show SonarQube issues relevant to the current context.

#### Synopsis

```
fm sonar
  [--project <key>]       if omitted, auto-detect from WI tags or project config
  [--severity <levels>]   e.g. MAJOR,CRITICAL,BLOCKER
  [--max <n>]             default: 20
  [--format markdown|json]
```

---

### `fm todo show`

**Goal:** List all child Tasks (todos) of the current User Story, grouped by state.

#### Synopsis

```
fm todo show
  [--all]              include closed/done items (default: open only)
  [--detail]           show description under each item
  [--format markdown|json]
```

#### Steps

1. Error if on baseline (no active WI).
2. Fetch child Tasks of current WI via ADO WIQL:
   `SELECT ... FROM WorkItemLinks WHERE Source.Id = {wi-id} AND LinkType = 'Child'`
3. Group by state: **Active** first, then **New**, then **Closed** (only if `--all`).

#### Output

```markdown
## Todos — #73240: login flow implementation

  ●  #73243  add tests                                       Active
  ○  #73241  implement login UI
  ○  #73244  update documentation

  ─────────────────────────────────────────
  0 done · 1 active · 2 open · 3 total
```

With `--all`:

```markdown
## Todos — #73240: login flow implementation

  ●  #73243  add tests                                       Active
  ○  #73241  implement login UI
  ○  #73244  update documentation
  ✓  #73242  write auth service                              Closed

  ─────────────────────────────────────────
  1 done · 1 active · 2 open · 4 total
```

With `--detail`:

```markdown
## Todos — #73240: login flow implementation

  ●  #73243  add tests                                       Active
             Writing unit tests for the auth middleware.

  ○  #73241  implement login UI
             Build React login form with validation and error states.

  ○  #73244  update documentation

  ─────────────────────────────────────────
  0 done · 1 active · 2 open · 3 total
```

**Legend:** `●` Active · `○` New · `✓` Closed

---

### `fm todo new`

**Goal:** Add a new child Task under the current User Story.

#### Synopsis

```
fm todo new
  --title <title>              (required)
  [--description <text>]
  [--assigned-to <email>]      default: unassigned
  [--pick]                     immediately set state to Active
  [--format markdown|json]
```

#### Steps

1. Error if on baseline.
2. Create ADO Task with `System.Title`, optional `System.Description`.
3. Link as child of current WI via ADO work item relations API.
4. If `--pick`: set state → Active.

#### Output

```markdown
## Todo Added — #73240: login flow implementation

  + #73245  write e2e tests
            (New · unassigned)

  0 done · 1 active · 3 open · 4 total
```

---

### `fm todo pick`

**Goal:** Set a todo to **Active**, marking it as currently in progress.

#### Synopsis

```
fm todo pick <ref>
  [--format markdown|json]
```

`<ref>` is a task ID, or a title fragment (case-insensitive substring match within the current WI's children). Error if the fragment matches more than one item — shows all matches.

#### Steps

1. Resolve `<ref>` to a child Task (see §2 — Todo resolution below).
2. Set state → **Active**.
3. Print updated todo list.

#### Output

```markdown
## Todo Active — #73245: write e2e tests

  ●  #73245  write e2e tests                                 Active ← just picked
  ○  #73241  implement login UI
  ○  #73244  update documentation

  0 done · 1 active · 2 open · 3 total
```

---

### `fm todo complete`

**Goal:** Mark a todo as **Closed**.

#### Synopsis

```
fm todo complete <ref>
  [--format markdown|json]
```

`<ref>` is a task ID or title fragment.

#### Steps

1. Resolve `<ref>` to a child Task.
2. Set state → **Closed**.
3. Print updated todo list (open items only).

#### Output

```markdown
## Todo Closed — #73245: write e2e tests

  ●  #73243  add tests                                       Active
  ○  #73241  implement login UI
  ○  #73244  update documentation
  ✓  #73245  write e2e tests                                 just closed

  ─────────────────────────────────────────
  1 done · 1 active · 2 open · 4 total
```

---

### `fm todo update`

**Goal:** Update a todo's title, description, or assignment.

#### Synopsis

```
fm todo update <ref>
  [--title <title>]
  [--description <text>]
  [--assigned-to <email>]
  [--state <state>]            Active, New, Closed
  [--format markdown|json]
```

`<ref>` is a task ID or title fragment.

#### Steps

1. Resolve `<ref>` to a child Task.
2. Apply provided fields via ADO WI PATCH.
3. Print the updated item in context of the full list.

#### Output

```markdown
## Todo Updated — #73241: implement login UI

  Title       implement login UI (React + validation)
  Description Build React login form with validation and error states.
  State       New
  Assigned    marco.denittis@synesthesia.it

  ─────────────────────────────────────────
  ●  #73243  add tests                                       Active
  ○  #73241  implement login UI (React + validation)         ← updated
  ○  #73244  update documentation
```

---

### `fm todo next`

**Goal:** Show the next open todo — the top-priority unstarted task.

#### Synopsis

```
fm todo next
  [--pick]       immediately set it to Active
  [--format markdown|json]
```

#### Steps

1. Fetch open todos (state = New), ordered by WI ID ascending (creation order).
2. Return the first one.
3. If `--pick`: set it Active.
4. If no open todos: show a summary and suggest `fm task complete` if all are closed.

#### Output

```markdown
## Next Todo — #73241: implement login UI (React + validation)

  Build React login form with validation and error states.

  ─────────────────────────────────────────
  0 done · 1 active · 2 open · 3 total
  Tip: run `fm todo pick 73241` to start it.
```

---

### `fm todo reopen`

**Goal:** Reopen a closed todo (set back to **New**).

#### Synopsis

```
fm todo reopen <ref>
  [--format markdown|json]
```

---

### Todo resolution (`<ref>`)

All `fm todo` subcommands that accept `<ref>` use the following resolution order:

1. **Exact numeric ID** (e.g. `73245`) — direct ADO lookup, validated as child of current WI.
2. **Title substring** (case-insensitive) — searches within the current WI's children.
   - Exactly one match: use it.
   - Multiple matches: list them and exit with an error asking for a more specific ref or the numeric ID.
   - No match: error with suggestion to run `fm todo show`.

---

### Todo in context output

`fm context` includes a compact todo summary when todos exist:

```markdown
### Todos
  ●  #73243  add tests                                       Active
  ○  #73241  implement login UI (React + validation)
  ○  #73244  update documentation
  ─────────────────────────────────────────
  0 done · 1 active · 2 open · 3 total  (run `fm todo show` for detail)
```

---

### `fm commit`

**Goal:** Commit staged (or all) changes in the current branch, transparently handling the `_docs` submodule when it has pending changes — committing the submodule first, then the parent repo with the updated pointer.

#### Synopsis

```
fm commit
  [--message <msg>]         commit message; if omitted, auto-generates from WI context
  [--all]                   stage all tracked changes before committing (git commit -a equivalent)
  [--amend]                 amend the last commit (message optional)
  [--docs-message <msg>]    override commit message for the _docs submodule commit
                            default: same as --message, prefixed with "docs: "
  [--no-docs]               skip submodule detection; commit only the main repo
  [--format markdown|json]
```

#### Submodule detection logic

Before committing the main repo, `fm commit` checks for pending `_docs` changes:

1. **`_docs` has uncommitted changes** (new files, modifications): commit `_docs` submodule first.
2. **`_docs` has unpushed commits**: push `_docs` first, then proceed.
3. **`_docs` is clean and up to date**: skip the submodule step entirely.

This detection is transparent — no flag required. Use `--no-docs` to bypass it explicitly.

#### Steps

1. Determine commit scope: staged only, or all tracked (`--all`).
2. **Check `_docs` submodule:**
   a. Run `git -C _docs status --porcelain` and `git -C _docs log @{u}..HEAD`.
   b. If the submodule has changes or unpushed commits:
      - If changes are unstaged: run `git -C _docs add -A`.
      - Commit with `--docs-message` (or `"docs: {message}"`).
      - Push `_docs`: `git -C _docs push`.
      - Stage the updated submodule pointer: `git add _docs`.
      - Print submodule commit info (hash, message).
3. **Commit the main repo** with the provided or auto-generated message.
4. Print commit summary.

#### Auto-generated message (no `--message`)

When in Activity context:
```
[#{wi-id}] {wi-title}: <descriptive summary of staged diff>
```
Example: `[#73240] login flow implementation: add auth service and unit tests`

When in Baseline context: message is required — error if omitted.

#### Output

```markdown
## Commit

  _docs  →  e3a1c2f  docs: add login flow notes
             pushed to mauto.wiki/main

  main   →  7b4d93a  [#73240] login flow implementation: add auth service
             branch: feature/73240-login-flow

  2 file(s) changed, 47 insertions(+), 3 deletions(-)
```

When no `_docs` changes:

```markdown
## Commit

  main   →  7b4d93a  [#73240] login flow implementation: add auth service
             branch: feature/73240-login-flow

  3 file(s) changed, 52 insertions(+)
```

With `--amend`:

```markdown
## Commit (amended)

  main   →  9f1e02b  [#73240] login flow implementation: add auth service and tests
             branch: feature/73240-login-flow  (force-push required)
  ⚠  Amended commit — run `fm push --force` to update the remote.
```

---

### `fm push`

**Goal:** Push the current branch to remote, with submodule-awareness and safety checks.

#### Synopsis

```
fm push
  [--force]              force push using --force-with-lease (errors if branch is protected)
  [--no-docs]            skip submodule push check
  [--format markdown|json]
```

#### Steps

1. **Check `_docs` for unpushed commits** (same detection as `fm commit`). Push `_docs` first if needed, then stage and commit the updated pointer automatically (using `"chore: update _docs submodule pointer"` as message).
2. Push current branch: `git push` (or `git push --force-with-lease` for `--force`).
3. Print push summary.

#### Output

```markdown
## Push

  _docs  →  pushed  (mauto.wiki/main · 1 commit ahead)

  main   →  feature/73240-login-flow
             3 commits pushed  ·  origin up to date
```

---

### `fm sync`

**Goal:** Commit all pending changes and push in one step. Shorthand for `fm commit --all && fm push`.

#### Synopsis

```
fm sync
  [--message <msg>]
  [--docs-message <msg>]
  [--format markdown|json]
```

#### Steps

1. Run `fm commit --all` logic (including submodule detection).
2. Run `fm push` logic.
3. Print combined summary.

#### Output

```markdown
## Sync

  _docs  →  e3a1c2f  docs: add login flow notes                 committed + pushed
  main   →  7b4d93a  [#73240] login flow: add auth service       committed + pushed
             feature/73240-login-flow  ·  4 commits ahead of main
```

If nothing to commit:

```markdown
## Sync

  Nothing to commit. Working tree clean.
  _docs  clean  ·  main  clean
```

---

### Submodule transparency rules (shared)

All three commands (`fm commit`, `fm push`, `fm sync`) apply the same `_docs` submodule handling:

| Submodule state | Action |
|-----------------|--------|
| Clean, up to date | Skip — no submodule step |
| Has uncommitted changes | `git add -A` + `git commit` in `_docs`, then push |
| Has unpushed commits only | Push `_docs` only |
| Both uncommitted + unpushed | Commit then push `_docs` |

After any `_docs` push, the parent repo's `_docs` pointer is automatically staged and included in the next main-repo commit (or a separate `"chore: update _docs submodule pointer"` commit if pushed standalone via `fm push`).

Use `--no-docs` on any command to bypass submodule detection and operate only on the main repo.

---

## 3. Overall Workflow

### Start of sprint / new feature

```
fm work new --title "login flow implementation" --branch "login-flow" --sonar-project mauto-chat-backend
```

→ WI created, branch created, draft PR created, WI Active, local branch checked out.

### Daily work loop

```bash
# Morning: understand where you are
fm context

# ... write code ...

# Save progress mid-day (commit + push, _docs handled transparently)
fm sync --message "wip: auth service skeleton"

# Keep branch up to date with main (check first, then merge or rebase)
fm task sync --check
fm task sync --rebase

# End of day: push and switch to baseline
fm task hold

# Next morning: restore context
fm work load 73240
# → stash restored, branch switched back
```

### Parallel review

```bash
# Someone asks you to review PR #15700
fm pr review 15700
# → current work stashed and pushed, moved to review branch

# Done reviewing, back to your work
fm work load 73240
# → stash restored, branch switched back
```

### Completion

```bash
# Work is done, ready for review
fm pr update --publish

# Trigger CI
fm pipeline run --id 614

# Check CI
fm pipeline status --watch

# Merge the PR (uses FM_MERGE_STRATEGY, default: squash)
fm pr merge

# Switch to main and pull
fm task complete
```

---

## 4. The 80/20 principle

`fm` is intentionally scoped. It covers the **~80% of daily workflow** that is repetitive, structured, and automatable. The remaining ~20% — edge cases, complex git operations, bulk ADO queries, policy overrides — is handled directly with the underlying tools.

### What `fm` covers (the 80%)

| Scenario | Command |
|----------|---------|
| Start new work | `fm work new` |
| Resume existing work | `fm work load` |
| Understand current state | `fm context` |
| Save and pause work | `fm task hold` / `fm sync` |
| Commit with submodule handling | `fm commit` / `fm sync` |
| Manage todos | `fm todo *` |
| Keep branch up to date | `fm task sync [--rebase]` |
| Publish PR for review | `fm pr update --publish` |
| Merge PR | `fm pr merge` |
| Trigger and watch CI | `fm pipeline run` / `fm pipeline status` |
| Finalize and return to baseline | `fm task complete` |

### What falls outside `fm` — use git, ado.cs, or ADO directly (the 20%)

`fm` does not attempt to cover these. When a situation is incongruent — broken links, unexpected WI states, diverged branches, policy exceptions — use the underlying tools directly. `fm` will never hide or abstract errors from these layers.

| Scenario | Direct tool |
|----------|-------------|
| Conflict resolution after `fm task sync` | `git mergetool`, `git rebase --continue` |
| Cherry-pick commits across branches | `git cherry-pick` |
| Interactive rebase, history cleanup | `git rebase -i` |
| Hard reset, branch deletion | `git reset --hard`, `git branch -D` |
| Search WIs with custom WIQL | `ado.cs wi search --query "..."` |
| Bulk WI updates | `ado.cs wi update` |
| Repair broken WI–branch–PR links manually | ADO web UI or `ado.cs` |
| Sonar deep-dive with pagination | `sonar.cs issues` |
| Creating child Tasks with full field control | `ado.cs wi create --type Task` |
| Pipeline YAML changes | file edit + `fm commit` |
| Policy bypasses, emergency overrides | ADO web UI |
| Inspecting raw ADO API responses | `ado.cs` with `--format json` |

**When in doubt:** run `fm context` to see the current state, then use `git` or `ado.cs` for anything `fm` cannot handle. The two layers are complementary, not competing.

---

## 5. Implementation notes (for later)

- `fm.cs` follows the same run-file pattern as `sonar.cs` and `ado.cs` (shebang `#!/usr/bin/dotnet run`, same NuGet packages).
- Reads all env vars from `.env`: `ADO_URL`, `ADO_PAT`, `ADO_PROJECT`, `SONAR_URL`, `SONAR_TOKEN`, and all `FM_*` variables.
- Shells out to `git` for local operations (`Process.Start`); uses ADO REST API for remote branch/PR/WI/link operations.
- State that cannot be derived from the branch name (e.g. stash names) is kept as git notes or a local `.fm-state.json` (gitignored).
- Default output format is **markdown** (unlike `ado.cs` which defaults to JSON).
- **Non-interactive by design:** safe to call from AI agents, CI scripts, and shell pipelines. Every command either succeeds or exits non-zero with a structured error message. No prompts, no confirmations, no TTY assumptions.
- **Idempotent by design:** all state-creating operations check for existing state first. A command that has already been partially executed can be re-run safely to complete it.
- **Link validation on every Activity command:** before executing, validate that the WI artifact links include the current branch and a PR. Repair missing links silently if possible; error and exit if the repair would require destructive action.
