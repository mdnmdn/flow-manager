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
| CI support | auto-detects Azure DevOps and GitHub Actions; populates config from pipeline env vars |
| Authentication | `auth login`, `auth logout`, `auth status`, `auth list` |

---

## Provider support

| Provider | Issue tracker | VCS / PRs | Pipelines | Status |
|---|---|---|---|---|
| Azure DevOps | ✅ | ✅ | ✅ | Primary target |
| GitHub | ✅ | ✅ | ✅ | Implemented |
| GitLab | — | — | — | Planned |

GitHub maps issues to work items (type encoded via `type:` labels), pull requests 1:1, and GitHub Actions workflows to pipelines.

---

## Configuration

Minimal `fm.toml` for Azure DevOps (credentials via environment variables):

```toml
[provider]
type = "ado"

[provider.ado]
url     = "https://dev.azure.com/myorg"
project = "myproject"
pat     = ""          # override with FM__PROVIDER__ADO__PAT
```

Minimal `fm.toml` for GitHub (PAT):

```toml
[provider]
type = "github"

[provider.github]
token = ""      # override with FM__PROVIDER__GITHUB__TOKEN
owner = "myorg"
repo  = "myrepo"
```

Minimal `fm.toml` for GitHub (App auth — no PAT needed):

```toml
[provider]
type = "github"

[provider.github]
owner   = "myorg"
repo    = "myrepo"
account = "default"   # which stored keychain account to use
```

Run `fm auth login` once after this and the token is stored in the OS keychain. No secrets in the config file.

In CI, ADO populates `url`/`project` from `SYSTEM_TEAMFOUNDATIONCOLLECTIONURI`/`SYSTEM_TEAMPROJECT`; GitHub populates `owner`/`repo` from `GITHUB_REPOSITORY` when left empty.

Run `fm init --discover` to auto-detect the provider from the git remote and generate a config.

---

## GitHub authentication

`fm` supports two authentication methods for GitHub in parallel. Token resolution order for every GitHub API call:

1. `token` in `[provider.github]` / `FM__PROVIDER__GITHUB__TOKEN` env var — explicit PAT
2. `GITHUB_TOKEN` env var — **automatically set by GitHub Actions**, zero config needed in pipelines
3. `GH_TOKEN` env var — alternate env var used by the `gh` CLI
4. OS keychain — token stored by `fm auth login` (GitHub App Device Flow)

### GitHub App Device Flow (local dev)

```bash
# Authenticate and store a token in the OS keychain
fm auth login

# Authenticate a second account (e.g. work vs personal)
fm auth login --account work

# Check status
fm auth status
fm auth status --account work

# List all stored accounts
fm auth list

# Remove an account
fm auth logout --account work
```

Per-project, tell fm which account to use:

```toml
[provider.github]
owner   = "my-org"
repo    = "my-repo"
account = "work"        # defaults to "default"
```

### App ID and Client ID injection

The GitHub App Client ID is resolved in this order:
- Compiled-in value (injected at build time via `GITHUB_CLIENT_ID` env var — for release binaries)
- Runtime `GITHUB_CLIENT_ID` env var (for contributors using their own dev App)
- `client_id` field in `[provider.github]` (per-project override)

For release CI (GitHub Actions):

```yaml
- name: Build release
  run: cargo build --release
  env:
    GITHUB_CLIENT_ID: ${{ secrets.GITHUB_APP_CLIENT_ID }}
    GITHUB_APP_ID: ${{ secrets.GITHUB_APP_ID }}
```

Contributors create their own GitHub App for local dev and export `GITHUB_CLIENT_ID` before running `fm auth login`.

### GitHub Actions pipelines

No configuration needed. `GITHUB_TOKEN` is automatically injected by the GitHub Actions runtime and picked up by `fm` without any `fm.toml` changes:

```yaml
- name: Run fm
  run: fm pr show
  # GITHUB_TOKEN is available automatically — no extra setup
```

### Linux system dependency

The OS keychain on Linux uses `libsecret`. Install the dev library before building:

```bash
# Debian / Ubuntu
sudo apt-get install libsecret-1-dev

# Fedora / RHEL
sudo dnf install libsecret-devel
```

---

## Install

```bash
cargo install --path .
```

Requires Rust 1.75+.
