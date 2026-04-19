# Bitbucket / Atlassian Provider Analysis for Flow Manager (`fm`)

This document analyzes the feasibility of implementing a Bitbucket provider for the Flow Manager (`fm`) tool, with particular attention to the Atlassian ecosystem integration (Jira as the Issue Tracker, Bitbucket Cloud/Server as the VCS provider).

## 1. Concept Mapping

| FM Domain / ADO Concept | Bitbucket / Atlassian Equivalent | Notes |
|-------------------------|----------------------------------|-------|
| **Work Item (WI)**      | **Jira Issue**                   | Jira is the standard Atlassian issue tracker; Bitbucket has a lightweight built-in issue tracker for simpler projects. |
| **User Story / Bug**    | **Jira Issue Type**              | Jira has formal `Story`, `Bug`, `Task`, `Epic` issue types. |
| **WI ID**               | **Jira Issue Key**               | Alphanumeric keys like `PROJ-123` (project key + number). Notably *not* a plain integer. |
| **WI State**            | **Jira Status / Workflow**       | Jira has fully configurable status workflows per project type. |
| **WI Tags**             | **Labels / Components**          | Jira supports Labels and Components; Components are structured (with owners). |
| **WI Assigned To**      | **Assignee (AccountId)**         | Jira uses account IDs (UUID) rather than usernames/emails in its v3 API. |
| **Pull Request (PR)**   | **Pull Request**                 | Bitbucket uses the same "Pull Request" term. Direct 1:1 mapping. |
| **Pipeline**            | **Bitbucket Pipelines**          | Defined in `bitbucket-pipelines.yml`; or Bamboo for enterprise. |
| **Pipeline Run**        | **Pipeline Result / Build**      | Each `bitbucket-pipelines.yml` execution is a "result". |
| **Artifact Links**      | **Jira Development Panel / Smart Commits** | Bitbucket integrates with Jira natively; branch/PR references appear in the Jira Development Panel automatically when using the Jira+Bitbucket integration. |
| **Child Work Items**    | **Jira Subtasks / Child Issues** | Jira has formal `Subtask` issue types as children of parent issues. |
| **Area / Iteration**    | **Epic / Sprint / Version**      | Jira has Epics, Sprints (Scrum), and Fix Versions for release tracking. |

## 2. Trait Compatibility Analysis

Actual trait signatures (from `src/providers/mod.rs`) and domain models (from `src/core/models.rs`) are referenced throughout this section.

### `IssueTracker` Trait

*   `get_work_item(id: i32)`, `update_work_item(id: i32, ...)`, `create_artifact_link(wi_id: i32, url: &str)`, `link_work_items(source_id: i32, target_id: i32, ...)`, `get_child_work_items(id: i32, ...)`: **Breaking incompatibility.** Every `IssueTracker` method takes `id: i32`. Jira issue keys are alphanumeric strings like `PROJ-123`. This is a **pervasive type-level conflict** — not just a branch-naming issue. Changing `i32` to a `WorkItemId(String)` newtype (or `String`) in the trait and in `WorkItem.id` is a prerequisite for any Jira integration.
*   `Context::Activity { wi_id: i32, ... }` in `src/core/context.rs` also uses `i32`, meaning the context resolution logic must be updated alongside the trait.
*   `query_work_items(wiql: &str)`: **Critical.** WIQL is ADO-specific. Jira uses **JQL** (`project = "PROJ" AND status = "In Progress" AND assignee = currentUser()`). JQL cannot use `username` or `userKey` — Jira Cloud v3 requires `accountId` UUIDs in queries (confirmed breaking change in Jira's API).
    *   *Suggestion:* Replace with a structured `WorkItemFilter`. Jira's REST API v3 supports structured filters; the provider builds a JQL string internally.
*   `create_artifact_link(wi_id: i32, url: &str)`: **Strong native support** once the ID type issue is resolved. The Jira Remote Links API (`POST /rest/api/3/issue/{issueIdOrKey}/remotelink`) handles explicit linking. The Jira–Bitbucket integration also auto-populates the Development Panel when the issue key appears in the branch name or commit (Smart Commits).
*   `link_work_items(source_id: i32, target_id: i32, relation: &str)`: Maps to Jira's Issue Link API (`POST /rest/api/3/issueLink`). Jira supports rich link types: `blocks`, `is blocked by`, `duplicates`, `relates to`, `clones`, etc. Again blocked by the `i32` ID type.
*   `get_child_work_items(id: i32, type)`: Maps to Jira Subtasks via a JQL query `parent = PROJ-123`. Blocked by the `i32` ID type.
*   `update_work_item_state(id: i32, state: &str)`: Maps to Jira transitions. Unlike ADO/GitLab where state is a direct field assignment, Jira requires a two-step process: `GET /transitions` to list available transitions from the current state, then `POST /transitions` with the transition ID. This must be hidden inside the implementation.
*   `WorkItem.assigned_to: Option<String>`: ADO and GitLab store email or display name. Jira Cloud v3 requires `accountId` (a UUID), not email. The `assigned_to` field must store the accountId, and a lookup step (email → accountId via `/rest/api/3/user/search`) is needed if users are specified by email.

### `VCSProvider` Trait

Bitbucket Cloud's REST API (v2.0) provides good coverage:

*   **PR Management:** `create_pull_request`, `update_pull_request`, `complete_pull_request` map to `/repositories/{workspace}/{repo_slug}/pullrequests` endpoints. Direct mapping.
*   **Merge Strategies:** FM's `MergeStrategy` enum has four variants; Bitbucket Cloud has three: `merge_commit` (standard `git merge --no-ff`), `fast_forward`, and `rebase_merge`. The mapping is:
    | FM `MergeStrategy`  | Bitbucket strategy    |
    |---------------------|-----------------------|
    | `NoFastForward`     | `merge_commit`        |
    | `Rebase`            | `fast_forward`        |
    | `RebaseMerge`       | `rebase_merge`        |
    | `Squash`            | *(not supported)*     |
    Bitbucket Cloud has **no squash merge strategy**. The `Squash` variant must either be rejected with an error or fall back to `merge_commit`.
*   **Draft PRs:** Bitbucket Cloud **does** support Draft PRs natively. Draft PRs prevent merging and suppress reviewer notifications until marked as ready. The `is_draft` flag maps directly.
*   **Reviewers:** Bitbucket uses `reviewers` array with `account_id` (UUID) — consistent with Jira's accountId requirement. The `add_reviewer(repository, id, reviewer_id: &str)` signature accommodates this as a string.
*   **Delete Source Branch:** Supported via `close_source_branch` field on PR creation/completion.

### `PipelineProvider` Trait

Bitbucket Pipelines API covers most needs but has a critical ID incompatibility:

*   `list_pipelines() -> Vec<Pipeline>`: The FM `Pipeline` struct has `{ id: i32, name: String, folder: String }`. Bitbucket pipelines are defined in `bitbucket-pipelines.yml` with no separate definition API. The provider can return a synthetic list of recent runs or named pipeline steps, but Bitbucket pipeline IDs are **UUIDs** (`pipeline_uuid`), not integers. `Pipeline.id: i32` is **incompatible**.
*   `run_pipeline(pipeline_id: i32, branch: &str) -> PipelineRun`: Maps to `POST /repositories/{workspace}/{repo_slug}/pipelines/` with a branch target. The `pipeline_id` parameter has no Bitbucket equivalent; the provider ignores it. However `PipelineRun.id: i32` cannot store a UUID.
*   `get_run_status(run_id: i32) -> PipelineRun`: Maps to `GET /repositories/{workspace}/{repo_slug}/pipelines/{pipeline_uuid}`. **Incompatible** — `run_id: i32` cannot represent a UUID.
*   Pipeline states (`PENDING`, `IN_PROGRESS`, `PAUSED`, `SUCCESSFUL`, `FAILED`, `ERROR`, `STOPPED`) must be mapped to FM's internal `PipelineRun.status: String` field.

For **Bamboo** (Atlassian's enterprise CI server), a separate implementation would be required using the Bamboo REST API. This is a significant scope addition and should be treated as a separate `BambooProvider`.

## 3. Critical Points & Challenges

### 1. Split Provider Architecture (Jira + Bitbucket)
The most significant architectural challenge: ADO is a monolithic platform where Work Items, VCS, and Pipelines share a single API surface. In the Atlassian ecosystem, these are typically **two separate products**:
- **Jira** for issue tracking (separate URL, auth, config)
- **Bitbucket** for VCS and Pipelines

The `Config` struct and provider factory must accommodate a "compound provider" where `IssueTracker` is backed by Jira and `VCSProvider` is backed by Bitbucket, each with independent credentials and URLs.

### 2. Jira Issue Key Format — Pervasive `i32` Incompatibility
Every `IssueTracker` trait method uses `id: i32`, and `WorkItem.id` and `Context::Activity.wi_id` are also `i32`. Jira issue keys are `PROJ-123` strings. This requires changing the ID type to `String` (or a newtype) across the trait, the `WorkItem` model, the `Context` enum, and `parse_wi_id_from_branch` in `src/core/context.rs`. This is the single largest required change for Jira support and touches every existing provider and command.

### 3. Jira State Transitions
Unlike direct state assignment (`update_work_item_state`), Jira requires a two-step process:
1.  `GET /transitions` to fetch available transitions from the current state.
2.  `POST /transitions` with the chosen transition ID.

The `IssueTracker` trait's `update_work_item_state(id, state)` signature implies direct assignment. The GitLab/GitHub providers can use this, but Jira needs the transition lookup step hidden inside the implementation.

### 4. Jira AccountId vs. Email
ADO and most providers use email addresses to identify users. Jira Cloud's v3 API uses opaque `accountId` UUIDs. The FM configuration and `add_reviewer` logic must handle a lookup step (email → accountId) or store accountIds directly.

### 5. Bitbucket Workspace Identifier
Bitbucket Cloud uses a `workspace` + `repo_slug` pair to identify repositories (e.g., `mycompany/my-repo`). This is analogous to GitHub's `owner/repo` and different from ADO's `organization/project/repo` triple.

### 6. Pipeline UUID Incompatibility
Bitbucket pipeline run IDs are UUIDs. Both `PipelineProvider::run_pipeline(pipeline_id: i32)` and `get_run_status(run_id: i32)`, as well as the `PipelineRun.id: i32` field in the domain model, are fundamentally incompatible with UUIDs. Supporting Bitbucket pipelines requires either changing `PipelineRun.id` to `String` or maintaining a UUID-to-integer mapping layer (not recommended).

### 7. No Squash Merge Strategy
FM's `MergeStrategy::Squash` variant has no equivalent in Bitbucket Cloud. The provider must either refuse this option with an explicit error or silently fall back to `merge_commit`. This should be surfaced via a `SupportedFeatures` capability mechanism (see Suggested Refactorings).

### 8. Bitbucket Server / Data Center
Enterprises may use Bitbucket Server (self-hosted) or Bitbucket Data Center. The API differs from Bitbucket Cloud in several ways. A separate implementation or strong abstraction would be needed. Scope for an initial implementation should target Bitbucket Cloud only.

### 9. Smart Commits and Jira Integration
The Jira–Bitbucket Smart Commits feature (e.g., `git commit -m "PROJ-123 #in-progress Fixed bug"`) provides a native mechanism that partially overlaps with FM's `create_artifact_link`. The provider can leverage this pattern by ensuring branch names and commit messages follow the Smart Commit format.

## 4. Suggested Refactorings

1.  **Compound Provider Support:**
    The provider factory must support composing an `IssueTracker` (Jira) with a `VCSProvider` (Bitbucket) independently. Introduce a `ProviderSet` struct that holds separate trait objects for each concern.

2.  **Generic WI ID Type (mandatory):**
    Change `WorkItem.id`, `Context::Activity.wi_id`, and every `IssueTracker` method parameter from `i32` to `String` (or a `WorkItemId(String)` newtype). Update `parse_wi_id_from_branch` to handle both `\d+` and `[A-Z]+-\d+` patterns. This change cascades through all existing providers and commands and should be done as a single preparatory refactor before adding any new provider.

3.  **State Transition Abstraction:**
    The `update_work_item_state` method signature can remain as-is, but the Jira implementation must internally perform the transition lookup. Add an optional `get_available_transitions(id)` method to the trait for providers that need it.

4.  **Search Abstraction:**
    Replace `query_work_items(wiql: &str)` with `query_work_items(filter: WorkItemFilter)`. For Jira, the provider builds a JQL string from the struct internally.

5.  **User Identity Abstraction:**
    Introduce a `UserId` enum (`Email(String)` | `AccountId(String)`) to handle both ADO/GitLab email-based identification and Jira's UUID-based system.

6.  **Capability Flags (`SupportedFeatures`):**
    Add a `SupportedFeatures` struct per provider to signal missing capabilities (e.g., `Squash` merge strategy). This allows FM command logic to gracefully degrade or report an error rather than sending an unsupported API call.

7.  **Pipeline ID type (mandatory):**
    Change `PipelineRun.id` and `Pipeline.id` from `i32` to `String` to accommodate Bitbucket's UUID-based pipeline identifiers. `PipelineProvider::run_pipeline` and `get_run_status` signatures must be updated accordingly.

## 5. Conclusion

Implementing a Bitbucket/Atlassian provider is **feasible but architecturally more complex** than GitHub or GitLab due to:

1.  The split between Jira (issue tracking) and Bitbucket (VCS/Pipelines) requiring a compound provider approach in the `Config` and factory layer.
2.  Jira's alphanumeric issue keys requiring **pervasive `i32` → `String` changes** across the `IssueTracker` trait, `WorkItem` model, `Context` enum, and all existing command logic.
3.  Jira's transition-based state model requiring multi-step state changes hidden inside the implementation.
4.  Bitbucket's UUID pipeline IDs requiring `PipelineRun.id` to change from `i32` to `String`.
5.  The `MergeStrategy::Squash` variant having no Bitbucket equivalent.

The Bitbucket VCS and Pipeline REST API surfaces are otherwise well-structured and map cleanly to the existing traits. Draft PRs are natively supported. The primary investment is in the Jira issue tracker integration, the compound provider architecture, and the two mandatory type-level changes (`WorkItemId` and `PipelineRunId`). Targeting **Bitbucket Cloud + Jira Cloud** with the v2/v3 REST APIs is recommended as the first milestone, deferring Bitbucket Server/Data Center and Bamboo support.
