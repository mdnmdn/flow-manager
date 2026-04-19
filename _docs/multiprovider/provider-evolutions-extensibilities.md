# Provider Evolution & Extensibility

This document describes the required evolutions to the flow-manager (`fm`) domain model, traits, and infrastructure to open the system to multiple providers — specifically GitLab, GitHub, and the Atlassian (Jira + Bitbucket) ecosystem — and by extension any future provider.

The analysis is based on the three provider analyses in this folder and on the current trait signatures in `src/providers/mod.rs`, the domain models in `src/core/models.rs`, and the configuration/context structures in `src/core/`.

-----

## 1. Mandatory Type-Level Changes

These are breaking changes that must be addressed before any new provider can be added. They are not provider-specific optimisations — they are prerequisites.

### 1.1 Work Item ID: `i32` → `WorkItemId`

**Current state:** every `IssueTracker` method, `WorkItem.id`, `Context::Activity.wi_id`, and `ContextManager` helpers all use `i32`.

**Problem:** Jira (and other trackers) identify issues with alphanumeric keys like `PROJ-123`. An `i32` cannot represent these.

**Required change:** introduce a `WorkItemId` newtype that wraps a `String`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct WorkItemId(pub String);

impl WorkItemId {
    pub fn from_int(n: i32) -> Self { WorkItemId(n.to_string()) }
    pub fn as_str(&self) -> &str { &self.0 }
}

impl std::fmt::Display for WorkItemId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
```

Every trait method and model field currently typed `i32` for a work item ID must be migrated to `WorkItemId`. This touches:

- `IssueTracker::get_work_item`, `update_work_item`, `update_work_item_state`, `create_artifact_link`, `link_work_items`, `get_child_work_items`
- `WorkItem.id`
- `Context::Activity.wi_id`
- `ContextManager::detect`, `resolve_id`, `derive_branch_name`

### 1.2 Pipeline & PR IDs: `i32` → `String`

**Problem:** Bitbucket pipeline run IDs are UUIDs. Hardcoding `i32` for `Pipeline.id`, `PipelineRun.id`, `PipelineProvider::run_pipeline(pipeline_id: i32)`, and `PipelineProvider::get_run_status(run_id: i32)` is incompatible.

PR IDs face the same concern for some providers (e.g. Bitbucket uses integer IDs, but the pattern should be consistent).

**Required change:** change numeric ID fields to `String` in the pipeline and PR models, and update the trait signatures accordingly:

```rust
pub struct Pipeline {
    pub id: String,   // was i32
    pub name: String,
    pub folder: String,
}

pub struct PipelineRun {
    pub id: String,   // was i32
    pub status: String,
    pub result: Option<String>,
    pub url: String,
}
```

Trait signatures:

```rust
async fn run_pipeline(&self, pipeline_id: &str, branch: &str) -> Result<PipelineRun>;
async fn get_run_status(&self, run_id: &str) -> Result<PipelineRun>;
```

The existing ADO provider, which uses integer IDs, simply formats/parses the integer as a string.

-----

## 2. Configuration Model

**Current state:** `Config` has a hard-wired `ado: AdoConfig` field, making any non-ADO provider structurally impossible to configure.

**Required change:** introduce a `provider` enum with per-variant sub-configs. The `ado` field must be replaced (or aliased during a transitional period).

```rust
#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ProviderConfig {
    Ado(AdoConfig),
    GitHub(GitHubConfig),
    GitLab(GitLabConfig),
    Atlassian(AtlassianConfig),  // Jira + Bitbucket compound
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    pub provider: ProviderConfig,
    pub sonar: Option<SonarConfig>,
    pub fm: FmConfig,
}
```

Provider-specific sub-configs:

```rust
pub struct AdoConfig {
    pub url: String,
    pub project: String,
    pub pat: String,
}

pub struct GitHubConfig {
    pub token: String,         // PAT or OAuth token
    pub owner: String,         // org or user
    pub repo: String,
    // base_url optional for GitHub Enterprise
    pub base_url: Option<String>,
}

pub struct GitLabConfig {
    pub token: String,
    pub namespace: String,     // group/project path
    pub project_id: Option<u64>,
    // supports self-hosted
    pub base_url: Option<String>,
}

pub struct AtlassianConfig {
    pub jira: JiraConfig,
    pub bitbucket: BitbucketConfig,
}

pub struct JiraConfig {
    pub url: String,
    pub email: String,
    pub api_token: String,
    pub project_key: String,
}

pub struct BitbucketConfig {
    pub workspace: String,
    pub repo_slug: String,
    pub token: String,
    // supports Bitbucket Server
    pub base_url: Option<String>,
}
```

A `base_url` field on each provider config is essential for self-hosted deployments (GitLab, Bitbucket Server, GitHub Enterprise).

-----

## 3. Provider Factory

**Current state:** providers appear to be instantiated directly.

**Required change:** introduce a factory or registry that selects and builds the correct provider set from the active config:

```rust
pub struct ProviderSet {
    pub issue_tracker: Box<dyn IssueTracker + Send + Sync>,
    pub vcs: Box<dyn VCSProvider + Send + Sync>,
    pub pipeline: Option<Box<dyn PipelineProvider + Send + Sync>>,
    pub quality: Option<Box<dyn QualityProvider + Send + Sync>>,
}

impl ProviderSet {
    pub fn from_config(config: &Config) -> Result<Self> {
        match &config.provider {
            ProviderConfig::Ado(c)        => Ok(AdoProviderSet::build(c)?),
            ProviderConfig::GitHub(c)     => Ok(GitHubProviderSet::build(c)?),
            ProviderConfig::GitLab(c)     => Ok(GitLabProviderSet::build(c)?),
            ProviderConfig::Atlassian(c)  => Ok(AtlassianProviderSet::build(c)?),
        }
    }
}
```

The Atlassian case is the important one: it requires a *compound* set where `IssueTracker` is backed by Jira and `VCSProvider`/`PipelineProvider` are backed by Bitbucket. The factory is the natural place to wire these independently.

Note that `pipeline` and `quality` are `Option<...>` — providers that do not offer these capabilities simply return `None`. The commands must check for presence and surface a clear error if the user invokes a pipeline command on a provider that does not expose pipelines.

-----

## 4. Work Item Search Abstraction

**Current state:** `IssueTracker::query_work_items(wiql: &str)` accepts a raw WIQL string — an ADO-specific query language.

**Required change:** replace the raw string with a provider-neutral `WorkItemFilter` struct:

```rust
#[derive(Debug, Default)]
pub struct WorkItemFilter {
    pub state: Option<String>,
    pub assigned_to: Option<String>,
    pub labels: Vec<String>,
    pub work_item_type: Option<String>,
    pub text: Option<String>,
    pub milestone: Option<String>,
    pub limit: Option<u32>,
}
```

The trait becomes:

```rust
async fn query_work_items(&self, filter: &WorkItemFilter) -> Result<Vec<WorkItem>>;
```

Each provider translates `WorkItemFilter` to its native query syntax internally:

- ADO → WIQL
- GitHub → search query string (`is:issue is:open label:feature`)
- GitLab → REST query parameters (`state`, `labels`, `milestone`, `assignee_username`)
- Jira → JQL string built from the struct

The command layer operates entirely on `WorkItemFilter` and is decoupled from the query language of any given provider.

-----

## 5. Context and Branch Name Parsing

**Current state:** `ContextManager::detect` parses `feature/{id}-slug` expecting `{id}` to be a parseable `i32`. The `derive_branch_name` function also formats the ID with `i32`.

**Required change:** generalise the parsing to handle both plain integers (ADO, GitHub, GitLab) and alphanumeric keys (Jira `PROJ-123`):

```rust
// Matches: feature/123-slug  OR  feature/PROJ-123-slug
static BRANCH_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(feature|fix)/([A-Z]+-\d+|\d+)-(.+)$").unwrap()
});
```

`ContextManager::detect` returns `Context::Activity { wi_id: WorkItemId, ... }` in both cases.

`derive_branch_name` must accept `WorkItemId` and produce a branch name that embeds the full key:

```
feature/PROJ-123-add-login-page
fix/456-null-pointer-crash
```

-----

## 6. Capability / Feature Flags

Not all providers support the same feature surface. Rather than discovering this at runtime (an API call fails), providers should declare their capabilities upfront.

**Proposed addition:** each provider implements a `capabilities()` method:

```rust
#[derive(Debug, Default)]
pub struct ProviderCapabilities {
    pub draft_pull_requests: bool,
    pub pipeline_support: bool,
    pub work_item_hierarchy: bool,       // parent/child relationships
    pub formal_artifact_links: bool,     // vs. description/comment-based linking
    pub merge_strategies: Vec<MergeStrategy>,
    pub work_item_relations: Vec<String>, // "blocks", "relates_to", "parent", etc.
}

pub trait CapableProvider {
    fn capabilities(&self) -> ProviderCapabilities;
}
```

Commands can check `capabilities` before attempting an unsupported operation and emit a clear, actionable message. Examples:

|Feature              |ADO|GitHub                   |GitLab           |Bitbucket        |
|---------------------|---|-------------------------|-----------------|-----------------|
|Draft PRs            |✓  |✓                        |✓                |✓                |
|Squash merge         |✓  |✓                        |✓                |✗                |
|Formal artifact links|✓  |✗ (comment-based)        |✓                |✓ (Jira panel)   |
|Pipeline support     |✓  |✓ (Actions)              |✓                |✓ (optional)     |
|Work item hierarchy  |✓  |partial (sub-issues beta)|partial (Premium)|✓ (Jira subtasks)|

-----

## 7. Optional / Degraded Features

Some behaviours cannot be identically implemented across providers and must be handled gracefully.

### 7.1 Draft Pull Requests

All current target providers support draft PRs. However, the `create_pull_request` signature already includes `is_draft: bool`. If a future provider does not support drafts, the capability flag (§6) should signal this, and the command should warn the user that the PR will be created as ready.

### 7.2 Artifact Linking

ADO has a formal Artifact Links API. GitHub and GitLab rely on mention/comment conventions or auto-close keywords. The `IssueTracker::create_artifact_link` contract is: *after this call, the work item is associated with the given URL*. The mechanism is provider-internal:

- **ADO:** REST artifact link API
- **GitLab:** Remote Links API or `Closes #n` keyword appended to MR description
- **GitHub:** standardised comment on the issue
- **Jira/Bitbucket:** Remote Links API; Smart Commits for implicit linking

The trait signature does not need to change. The difference in fidelity (a formal link vs. a comment) is a provider implementation detail.

### 7.3 Pipeline Definitions

ADO Pipelines have named definitions with integer IDs. GitLab and Bitbucket have a single `*.yml` file per project. For providers without named definitions:

- `list_pipelines()` returns a synthetic single entry (or recent run history)
- `run_pipeline(pipeline_id, branch)` ignores `pipeline_id` and triggers on `branch`
- The documentation and `capabilities()` should make this clear

### 7.4 Work Item State Transitions

ADO and GitLab allow direct state assignment. Jira requires a two-step lookup (list transitions → apply transition ID). The `update_work_item_state(id, state)` interface remains unchanged. Jira’s implementation hides the transition-lookup step internally. Optionally, the trait can expose a discovery method:

```rust
// Optional: providers that don't need this can return an empty vec
async fn available_states(&self, id: &WorkItemId) -> Result<Vec<String>> {
    Ok(vec![]) // default implementation
}
```

### 7.5 MergeStrategy Gaps

`MergeStrategy::Squash` is not supported by Bitbucket. `MergeStrategy::Rebase` (pure rebase, no merge commit) has no GitLab equivalent. The `capabilities().merge_strategies` field (§6) declares what each provider supports. If the configured strategy is unsupported, the command should fail early with a clear error rather than sending an unsupported API call.

### 7.6 User Identity

ADO and GitLab identify users by email or display name. Jira Cloud’s v3 API requires opaque `accountId` UUIDs. Introduce a `UserId` type:

```rust
pub enum UserId {
    Email(String),
    AccountId(String),   // Jira Cloud UUID
    Username(String),    // GitHub login, GitLab username
}
```

The `WorkItem.assigned_to` field and `add_reviewer` argument should accept a `String` that each provider interprets as appropriate for its identity model. Where a lookup is needed (email → accountId for Jira), the provider performs it internally.

-----

## 8. Compound Provider (Atlassian)

The Atlassian ecosystem splits responsibilities: Jira owns issue tracking and Bitbucket owns VCS/Pipelines. This requires a compound provider where `IssueTracker` and `VCSProvider` are backed by different services with independent credentials and base URLs.

The `ProviderSet` struct (§3) already accommodates this: the factory for `AtlassianConfig` simply instantiates `JiraIssueTracker` and `BitbucketVcsProvider` independently and places them in the set.

The key implication: **the `IssueTracker` and `VCSProvider` traits must not share state or assume they originate from the same API surface.** This is already true structurally but should be confirmed in practice — no method on `VCSProvider` should take a work item ID, and vice versa.

-----

## 9. Summary of Required Changes

The table below distinguishes between **mandatory** changes (without which a new provider cannot function) and **recommended** changes (which improve the architecture but are not strictly blocking for all providers):

|Change                                                                |Scope                                    |Priority                 |
|----------------------------------------------------------------------|-----------------------------------------|-------------------------|
|`WorkItemId` newtype replaces `i32` in `IssueTracker` trait and models|`IssueTracker`, `models.rs`, `context.rs`|**Mandatory** (Jira)     |
|`Pipeline.id` and `PipelineRun.id` to `String`                        |`models.rs`, `PipelineProvider`          |**Mandatory** (Bitbucket)|
|Generic `ProviderConfig` enum replaces `ado: AdoConfig`               |`config.rs`                              |**Mandatory** (all)      |
|Provider factory / `ProviderSet`                                      |`src/providers/`                         |**Mandatory** (all)      |
|`WorkItemFilter` replaces `wiql: &str`                                |`IssueTracker` trait, commands           |**Mandatory** (all)      |
|Branch name parser handles alphanumeric IDs                           |`context.rs`                             |**Mandatory** (Jira)     |
|`ProviderCapabilities` struct and `capabilities()`                    |`src/providers/mod.rs`                   |Recommended              |
|`available_states()` on `IssueTracker`                                |`IssueTracker` trait                     |Recommended (Jira)       |
|`UserId` enum for user identity                                       |`models.rs`, trait signatures            |Recommended (Jira)       |
|`base_url` on every provider config                                   |`config.rs`                              |Recommended (self-hosted)|
|`pipeline` and `quality` as `Option<...>` in `ProviderSet`            |`src/providers/`                         |Recommended              |

-----

## 10. Migration Path

To avoid a big-bang rewrite, the changes can be sequenced as follows:

**Phase 1 — Type foundations** (no functional changes, all providers still work)

- Introduce `WorkItemId(String)` and migrate all `i32` work-item ID usages
- Change `Pipeline.id` and `PipelineRun.id` to `String`
- Update ADO provider to format/parse these as before

**Phase 2 — Config & factory**

- Introduce `ProviderConfig` enum; keep `ado` as a supported variant
- Build `ProviderSet` and factory; wire existing ADO provider through it
- All existing behaviour is unchanged

**Phase 3 — Search abstraction**

- Introduce `WorkItemFilter`; update ADO provider to build WIQL internally
- Update commands to construct `WorkItemFilter` instead of raw strings

**Phase 4 — Capability flags**

- Implement `ProviderCapabilities` on the ADO provider
- Add capability checks to commands that use optional features

**Phase 5 — New provider implementations**

- Implement GitHub, GitLab, and Atlassian provider sets behind the existing trait surface
- Each phase can ship and be tested independently
