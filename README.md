# flow-manager (`fm`)

> **Early stage.** Interfaces and behaviours are evolving.

An opinionated CLI for development workflows — built for both humans and AI agents working side by side on the same codebase.

`fm` wraps Azure DevOps, Git, and SonarQube behind a small set of high-level commands that enforce consistent branch naming, work item linking, and PR hygiene. It removes the ceremony so you can focus on the work.

---

## Goals

### Development sugar with task tooling

`fm` treats a work item, its branch, and its pull request as a single unit — an **Activity**. Creating a task, branching, pushing, and opening a draft PR are one command. Holding, syncing, and closing follow the same pattern.

```
fm task new "add retry logic to pipeline poller"
fm task load 73235
fm task sync
fm task complete
```

The tool knows where you are by reading the current branch. No flags, no IDs to remember mid-flow.

### Review helpers for human / AI collaboration

`fm` provides a structured bridge between AI-generated code review and Azure DevOps PR threads:

- `fm pr show` produces a self-contained `context.md` document — the PR description, all threads, changed files, and optional project context — ready to feed to an AI agent.
- The agent writes a `review.yaml` (or `review.md`) with structured feedback: thread replies, new inline comments, open point resolutions, and an overall recommendation.
- `fm pr feedback validate` checks the review file before anything touches the API.
- `fm pr feedback apply` executes it: replies, resolves threads, posts new inline comments, all in one deterministic pass.

```
fm pr show --out context.md
# hand context.md to your AI agent, receive review.yaml back
fm pr feedback validate --file review.yaml
fm pr feedback apply --file review.yaml
```

---

## Current scope

| Area | Commands |
|---|---|
| Task lifecycle | `task new`, `task load`, `task list`, `task show`, `task hold`, `task complete`, `task sync`, `task update`, `task comment` |
| PR management | `pr show`, `pr update`, `pr merge`, `pr review`, `pr comment` |
| PR threads | `pr thread list`, `pr thread reply`, `pr thread resolve` |
| AI review | `pr feedback validate`, `pr feedback apply`, `pr feedback structure`, `pr feedback schema` |
| Todo list | `todo show`, `todo new`, `todo pick`, `todo complete`, `todo next` |
| Pipeline | `pipeline run`, `pipeline status` |
| Code quality | `sonar` |
| Git sugar | `commit`, `push`, `sync` |
| CI support | auto-detects Azure DevOps pipelines; populates config from pipeline env vars |

---

## Provider support

| Provider | Issue tracker | VCS / PRs | Status |
|---|---|---|---|
| Azure DevOps | ✅ | ✅ | Primary target |
| GitHub | — | — | Planned |
| GitLab | — | — | Planned |

---

## Configuration

Minimal `fm.toml` (credentials via environment variables):

```toml
[provider]
type = "ado"

[provider.ado]
url     = "https://dev.azure.com/myorg"
project = "myproject"
pat     = ""          # override with FM__PROVIDER__ADO__PAT
```

In CI, `url` and `project` are auto-populated from `SYSTEM_TEAMFOUNDATIONCOLLECTIONURI` and `SYSTEM_TEAMPROJECT` when left empty.

---

## Install

```bash
cargo install --path .
```

Requires Rust 1.75+.
