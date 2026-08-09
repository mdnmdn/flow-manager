# CI Mode — Design Proposal

## Problem Statement

Flow Manager (`fm`) currently assumes it is run interactively by a developer on a local machine. Context (Baseline vs Activity) is determined by reading the current git branch via local git commands, and credentials are injected through a committed config file or a local `.env` file.

In a CI/CD pipeline, none of these assumptions hold:

- The git checkout may be **detached HEAD** or shallow-cloned, making `git rev-parse --abbrev-ref HEAD` unreliable.
- PR context (source branch, target, PR ID) is available **only** from pipeline-injected environment variables, not from local git state.
- Secrets (PAT, tokens) are managed by the pipeline secret store and injected as environment variables — there is no `.env` file.
- The tool must operate non-interactively: no prompts, deterministic output, exit codes that signal success/failure to the pipeline.

CI mode solves this by:

1. **Detecting** at startup whether `fm` is running inside a supported pipeline.
2. **Injecting context** from pipeline environment variables (branch, PR ID) rather than from local git state.
3. **Allowing manual override** via a `[ci]` config block for edge cases.

---

## Scope

This proposal covers:

- Provider-agnostic detection interface.
- Azure DevOps pipeline detection and variable mapping (first supported provider).
- A new `CiEnvironment` abstraction surfacing normalised context to the rest of the tool.
- A `[ci]` config section for manual overrides.
- Required code changes across `src/core/`, `src/main.rs`, and `src/core/config.rs`.

GitHub Actions and GitLab CI detection are deferred; the architecture is designed to accommodate them with minimal changes.

---

## Azure DevOps Pipeline Environment Variables

Azure DevOps Pipelines inject a rich set of predefined variables. The following are relevant for CI mode.

### Detection variable

| Variable   | Value when in pipeline | Notes                              |
|------------|------------------------|------------------------------------|
| `TF_BUILD` | `True`                 | Always present in ADO agent jobs.  |

This is the canonical detector. No other CI system sets `TF_BUILD`.

### Branch and source context

| Variable                              | Example value                              | Notes                                        |
|---------------------------------------|--------------------------------------------|----------------------------------------------|
| `BUILD_SOURCEBRANCH`                  | `refs/heads/feature/73235-my-task`         | Full ref of the trigger branch.              |
| `BUILD_SOURCEBRANCHNAME`              | `feature/73235-my-task`                    | Branch name without `refs/heads/` prefix.    |
| `BUILD_BUILDID`                       | `1042`                                     | Unique build identifier.                     |
| `BUILD_BUILDNUMBER`                   | `20240421.3`                               | Human-readable build number.                 |
| `BUILD_REASON`                        | `PullRequest`, `IndividualCI`, `Manual`, … | Trigger type.                                |

### Pull request context (only set in PR builds)

| Variable                                    | Example value                             | Notes                                          |
|---------------------------------------------|-------------------------------------------|------------------------------------------------|
| `SYSTEM_PULLREQUEST_PULLREQUESTID`          | `456`                                     | ADO internal PR ID.                            |
| `SYSTEM_PULLREQUEST_PULLREQUESTNUMBER`      | `78`                                      | PR display number (may differ from ID).        |
| `SYSTEM_PULLREQUEST_SOURCEBRANCH`           | `refs/heads/feature/73235-my-task`        | Source (head) branch of the PR.                |
| `SYSTEM_PULLREQUEST_TARGETBRANCH`           | `refs/heads/main`                         | Target (base) branch of the PR.                |
| `SYSTEM_PULLREQUEST_SOURCEREPOSITORYURI`    | `https://dev.azure.com/org/proj/_git/repo`| Source repo URI (useful for fork PRs).         |

### Organisation context

| Variable                              | Example value                              | Notes                                        |
|---------------------------------------|--------------------------------------------|----------------------------------------------|
| `SYSTEM_TEAMFOUNDATIONCOLLECTIONURI`  | `https://dev.azure.com/myorg/`             | Can substitute for `provider.ado.url`.       |
| `SYSTEM_TEAMPROJECT`                  | `my-project`                               | Can substitute for `provider.ado.project`.   |
| `BUILD_REPOSITORY_NAME`               | `my-repo`                                  | Repository name.                             |

### How to determine a PR vs a branch build

```
BUILD_REASON == "PullRequest"
  → SYSTEM_PULLREQUEST_PULLREQUESTID is set
  → SYSTEM_PULLREQUEST_SOURCEBRANCH is the working branch

BUILD_REASON != "PullRequest"
  → BUILD_SOURCEBRANCHNAME is the working branch
  → no PR variables are set
```

---

## Detection Architecture

CI detection is provider-specific at the environment level but exposes a normalised struct to the rest of the tool.

### Normalised output: `CiEnvironment`

```rust
// src/core/ci.rs

pub enum CiPlatform {
    AzureDevOps,
    // future: GitHubActions, GitLabCi, Jenkins, …
}

/// Normalised view of the CI environment, regardless of platform.
pub struct CiEnvironment {
    pub platform: CiPlatform,
    /// Working branch name (no refs/heads/ prefix).
    pub branch: String,
    /// PR ID when triggered by a pull request.
    pub pr_id: Option<String>,
    /// PR source branch (same as `branch` in most cases).
    pub pr_source_branch: Option<String>,
    /// PR target branch (e.g. "main").
    pub pr_target_branch: Option<String>,
    /// Platform-specific build identifier.
    pub build_id: Option<String>,
}
```

### Detection function

```rust
impl CiEnvironment {
    /// Returns `Some` when a known CI environment is detected, `None` otherwise.
    pub fn detect() -> Option<Self> {
        Self::detect_azure_devops()
        // future: .or_else(Self::detect_github_actions)
        //         .or_else(Self::detect_gitlab_ci)
    }

    fn detect_azure_devops() -> Option<Self> {
        // TF_BUILD=True is the canonical ADO signal.
        if std::env::var("TF_BUILD").ok().as_deref() != Some("True") {
            return None;
        }

        // Prefer PR source branch when available.
        let branch = std::env::var("SYSTEM_PULLREQUEST_SOURCEBRANCH")
            .ok()
            .or_else(|| std::env::var("BUILD_SOURCEBRANCHNAME").ok())?;
        let branch = branch.trim_start_matches("refs/heads/").to_string();

        let pr_id = std::env::var("SYSTEM_PULLREQUEST_PULLREQUESTID").ok();
        let pr_source_branch = std::env::var("SYSTEM_PULLREQUEST_SOURCEBRANCH")
            .ok()
            .map(|b| b.trim_start_matches("refs/heads/").to_string());
        let pr_target_branch = std::env::var("SYSTEM_PULLREQUEST_TARGETBRANCH")
            .ok()
            .map(|b| b.trim_start_matches("refs/heads/").to_string());
        let build_id = std::env::var("BUILD_BUILDID").ok();

        Some(CiEnvironment {
            platform: CiPlatform::AzureDevOps,
            branch,
            pr_id,
            pr_source_branch,
            pr_target_branch,
            build_id,
        })
    }
}
```

---

## Context Injection in CI Mode

When a `CiEnvironment` is detected, context determination bypasses the local git branch read and uses the injected values instead.

### Decision tree

```
CiEnvironment detected?
│
├─ NO  → standard interactive mode (git branch + BranchCache)
│
└─ YES →
       pr_id present?
       │
       ├─ YES → CiContext::PullRequest { pr_id, source_branch, target_branch }
       │         ↓
       │         ContextManager::detect(source_branch)
       │         → Activity { wi_id, wi_type } if branch follows convention
       │         → Baseline               if not (unusual but possible)
       │
       └─ NO  → CiContext::Branch { branch }
                 ↓
                 ContextManager::detect(branch)
                 → same logic as interactive mode
```

### New enum: `CiContext`

```rust
// src/core/ci.rs

pub enum CiContext {
    /// Running in a PR pipeline build.
    PullRequest {
        pr_id: String,
        source_branch: String,
        target_branch: String,
    },
    /// Running on a plain branch build (no PR).
    Branch {
        branch: String,
    },
}

impl CiContext {
    pub fn from_environment(env: &CiEnvironment, overrides: &CiConfig) -> Self {
        // Config overrides take highest priority.
        if let Some(pr_id) = overrides.pr_id.clone().or_else(|| env.pr_id.clone()) {
            return CiContext::PullRequest {
                pr_id,
                source_branch: overrides
                    .branch
                    .clone()
                    .unwrap_or_else(|| env.branch.clone()),
                target_branch: env.pr_target_branch.clone().unwrap_or_default(),
            };
        }
        CiContext::Branch {
            branch: overrides.branch.clone().unwrap_or_else(|| env.branch.clone()),
        }
    }

    /// The branch name that should be used for `ContextManager::detect`.
    pub fn working_branch(&self) -> &str {
        match self {
            CiContext::PullRequest { source_branch, .. } => source_branch,
            CiContext::Branch { branch } => branch,
        }
    }
}
```

---

## Configuration Override: `[ci]` Block

A `[ci]` section in `fm.toml` (or environment variables) allows manual override of every detected value. This is useful when:

- The pipeline uses a custom checkout strategy that changes env var values.
- Testing CI behaviour locally without a real pipeline.
- Edge cases like fork PRs where branch names differ between repos.

### Config struct

```rust
// src/core/config.rs (addition)

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct CiConfig {
    /// Force CI mode even when not in a real pipeline.
    #[serde(default)]
    pub enabled: bool,
    /// Override the detected branch name.
    pub branch: Option<String>,
    /// Override the detected PR ID.
    pub pr_id: Option<String>,
    /// Override the detected PR target branch.
    pub pr_target_branch: Option<String>,
}
```

Add to `Config`:

```rust
pub struct Config {
    pub provider: Option<ProviderConfig>,
    pub sonar: Option<SonarConfig>,
    pub fm: FmConfig,
    #[serde(default)]
    pub ci: CiConfig,   // NEW
}
```

### TOML example

```toml
[ci]
enabled = false          # true to force CI mode locally
branch  = ""             # override branch name (empty = auto-detect)
pr_id   = ""             # override PR ID (empty = auto-detect)
```

### Environment variable mapping

| Config key           | Environment variable        |
|----------------------|-----------------------------|
| `ci.enabled`         | `FM__CI__ENABLED`           |
| `ci.branch`          | `FM__CI__BRANCH`            |
| `ci.pr_id`           | `FM__CI__PR_ID`             |
| `ci.pr_target_branch`| `FM__CI__PR_TARGET_BRANCH`  |

These have the highest precedence (overriding even the pipeline's own variables), so they are suitable for pipeline-level variable group overrides.

---

## ADO Provider Config Auto-Population

When running in an ADO pipeline, several config values can be inferred from environment variables, reducing the need to repeat them in `fm.toml`:

| `fm` config key        | ADO env variable                      | Condition           |
|------------------------|---------------------------------------|---------------------|
| `provider.ado.url`     | `SYSTEM_TEAMFOUNDATIONCOLLECTIONURI`  | if not set in file  |
| `provider.ado.project` | `SYSTEM_TEAMPROJECT`                  | if not set in file  |

The `FM__` prefix env vars already handle this for credentials (`FM__PROVIDER__ADO__PAT`). The ADO-specific variables are a convenience fallback so that a minimal `fm.toml` only needs the PAT:

```toml
[provider]
type = "ado"

[provider.ado]
pat = "PLACEHOLDER"  # overridden by FM__PROVIDER__ADO__PAT in CI
```

Implementation note: this auto-population should happen inside `Config::load()` after normal loading, as a post-processing step — not by adding `SYSTEM_TEAMFOUNDATIONCOLLECTIONURI` directly to the `config` crate sources (to avoid coupling the loader to ADO-specific logic).

---

## Startup Sequence in CI Mode

```
main()
  │
  ├─ dotenvy::dotenv().ok()
  ├─ Config::load()                          ← loads fm.toml + FM__ env vars
  ├─ CiEnvironment::detect()                 ← checks TF_BUILD etc.
  │
  ├─ if ci_env.is_some() || config.ci.enabled:
  │     CiContext::from_environment(&ci_env, &config.ci)
  │       → working_branch()
  │       → ContextManager::detect(branch)   ← existing branch→context logic
  │     print "[CI mode: Azure DevOps | build 1042 | PR #456]" to stderr
  │
  └─ dispatch command with resolved Context
```

The CI mode banner written to stderr is informational only and does not affect stdout output used by downstream pipeline tasks.

---

## Required Code Changes

### New file: `src/core/ci.rs`

Contains:
- `CiPlatform` enum
- `CiEnvironment` struct + `detect()` / `detect_azure_devops()` methods
- `CiContext` enum + `from_environment()` / `working_branch()` methods

Export from `src/core/mod.rs` (or `src/lib.rs`).

### Modified: `src/core/config.rs`

- Add `CiConfig` struct (shown above).
- Add `ci: CiConfig` field to `Config`.
- Add post-load ADO variable fallback in `Config::load()`:

```rust
pub fn load() -> Result<Self, ConfigError> {
    let mut cfg: Config = ConfigLoader::builder()
        // ... existing sources ...
        .build()?
        .try_deserialize()?;

    // ADO convenience fallback: populate url/project from pipeline vars if absent.
    if let Some(ref mut provider) = cfg.provider {
        if provider.kind == "ado" {
            if let Some(ref mut ado) = provider.ado {
                if ado.url.is_empty() {
                    if let Ok(uri) = std::env::var("SYSTEM_TEAMFOUNDATIONCOLLECTIONURI") {
                        ado.url = uri.trim_end_matches('/').to_string();
                    }
                }
                if ado.project.is_empty() {
                    if let Ok(proj) = std::env::var("SYSTEM_TEAMPROJECT") {
                        ado.project = proj;
                    }
                }
            }
        }
    }

    Ok(cfg)
}
```

### Modified: `src/main.rs`

After `Config::load()`, detect CI environment and derive context:

```rust
let config = fm::core::config::Config::load()?;
let ci_env = fm::core::ci::CiEnvironment::detect()
    .or_else(|| config.ci.enabled.then(CiEnvironment::forced));
let ci_ctx = ci_env.as_ref().map(|env| CiContext::from_environment(env, &config.ci));

if let Some(ref ctx) = ci_ctx {
    eprintln!("[fm ci] {} | build {} | branch: {}{}",
        ctx.platform_label(),
        ci_env.as_ref().and_then(|e| e.build_id.as_deref()).unwrap_or("?"),
        ctx.working_branch(),
        ci_ctx.as_ref().and_then(|c| c.pr_id()).map(|id| format!(" | PR #{id}")).unwrap_or_default(),
    );
}
```

The resolved `ci_ctx` is then threaded into commands that call `ContextManager::detect()`, replacing the current branch read when in CI mode.

### Modified: `src/core/context.rs`

`ContextManager::detect` currently takes `branch: &str` already — no signature change needed. The caller in each command simply passes `ci_ctx.working_branch()` instead of the local git branch when in CI mode.

### Modified: `src/providers/git.rs`

The `get_current_branch()` method should short-circuit in CI mode. One approach: accept an `Option<&str>` override parameter, or thread the `ci_ctx` into `LocalGitProvider`. A cleaner approach is to keep `get_current_branch()` unchanged and let the command layer decide which source to use.

---

## Edge Cases

| Scenario | Behaviour |
|---|---|
| Detached HEAD in CI | Use `BUILD_SOURCEBRANCHNAME` instead of git output. |
| Shallow clone | Branch name from env vars, no local commit history needed. |
| Fork PR | `SYSTEM_PULLREQUEST_SOURCEREPOSITORYURI` differs from target repo. ADO API calls still target the target repo. Log a warning. |
| Branch doesn't match convention | `ContextManager::detect()` returns `Baseline`; most write commands (task complete, PR merge) will be no-ops or error cleanly. |
| Manual local test | Set `FM__CI__ENABLED=true FM__CI__BRANCH=feature/123-foo` to simulate CI locally. |
| `TF_BUILD` set but malformed variables | Detection returns `None` (treated as non-CI); log a warning if `TF_BUILD=True` but required vars are missing. |

---

## Non-Goals

- Interactive mode changes: CI mode is entirely additive and does not affect normal interactive usage.
- Automatic pipeline YAML generation.
- GitHub Actions or GitLab CI detection (deferred, architecture is ready).
- Changing command semantics in CI mode: commands behave identically; only context sourcing changes.

---

## Future Work

- `fm ci status` — a read-only command suitable for pipeline reporting steps.
- GitHub Actions detection (`GITHUB_ACTIONS=true`, `GITHUB_REF`, `GITHUB_HEAD_REF`).
- GitLab CI detection (`GITLAB_CI=true`, `CI_COMMIT_BRANCH`, `CI_MERGE_REQUEST_IID`).
- Jenkins detection (`JENKINS_URL`, `BRANCH_NAME`, `CHANGE_ID`).
- Structured JSON output mode (`--output json`) for machine consumption in pipelines.
