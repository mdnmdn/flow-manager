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

### `IssueTracker` Trait

*   `query_work_items(wiql: &str)`: **Critical.** WIQL is ADO-specific. Jira uses its own query language **JQL (Jira Query Language)**, which is also a SQL-like DSL (e.g., `project = "PROJ" AND status = "In Progress" AND assignee = currentUser()`). While JQL is more powerful and widespread than WIQL, it is still provider-specific.
    *   *Suggestion:* Replace with a structured `WorkItemFilter`. Jira's REST API v3 also supports structured filters via `POST /rest/api/3/issue/picker` or `POST /rest/api/3/jql/parse`, so a struct-based approach is viable.
*   `create_artifact_link(wi_id, url)`: **Strong native support.** The Jira–Bitbucket integration automatically populates the Development Panel (branches, PRs, commits) when the Jira issue key appears in the branch name or commit message (Smart Commits). The provider can also use the Jira Remote Links API (`POST /rest/api/3/issue/{issueIdOrKey}/remotelink`) for explicit linking.
*   `link_work_items(source_id, target_id, relation)`: Maps to Jira's Issue Link API (`POST /rest/api/3/issueLink`). Jira supports rich link types: `blocks`, `is blocked by`, `duplicates`, `relates to`, `clones`, etc., defined per Jira instance.
*   `get_child_work_items(id, type)`: Maps to Jira Subtasks (querying issues with `parent = PROJ-123`) or child issues in next-gen projects. Easily implemented via a JQL query.
*   `update_work_item_state(id, state)`: Maps to Jira transitions (`POST /rest/api/3/issue/{issueIdOrKey}/transitions`). Unlike ADO/GitLab where state is set directly, Jira requires fetching available transitions first and posting a transition ID.

### `VCSProvider` Trait

Bitbucket Cloud's REST API (v2.0) provides good coverage:

*   **PR Management:** `create_pull_request`, `update_pull_request`, `complete_pull_request` map to `/repositories/{workspace}/{repo_slug}/pullrequests` endpoints. Direct mapping.
*   **Merge Strategies:** `MergeStrategy` maps to Bitbucket's merge strategies: `merge_commit`, `squash`, `fast_forward`. The `rebase_merge` strategy exists only via `fast_forward` in Bitbucket.
*   **Draft PRs:** Bitbucket Cloud does not have a formal "Draft PR" feature (as of 2024). A workaround is a `[WIP]` prefix in the title, which lacks API-level enforcement.
*   **Reviewers:** Bitbucket uses `reviewers` array with `account_id` or `uuid`, similar to GitHub/GitLab.
*   **Delete Source Branch:** Supported via `close_source_branch` field on PR creation/completion.

### `PipelineProvider` Trait

Bitbucket Pipelines API covers the core needs:

*   `list_pipelines`: Maps to `/repositories/{workspace}/{repo_slug}/pipelines/` (list of runs). For pipeline *definitions*, pipelines are defined as named steps in `bitbucket-pipelines.yml`; there is no separate API for listing definitions.
*   `run_pipeline`: Maps to `POST /repositories/{workspace}/{repo_slug}/pipelines/` with a target specifying branch/commit.
*   `get_pipeline_run`: Maps to `/repositories/{workspace}/{repo_slug}/pipelines/{pipeline_uuid}`.
*   `get_run_status`: Pipeline states (`PENDING`, `IN_PROGRESS`, `PAUSED`, `SUCCESSFUL`, `FAILED`, `ERROR`, `STOPPED`) must be mapped to FM's internal status model.

For **Bamboo** (Atlassian's enterprise CI server), a separate implementation would be required using the Bamboo REST API. This is a significant scope addition and should be treated as a separate `BambooProvider`.

## 3. Critical Points & Challenges

### 1. Split Provider Architecture (Jira + Bitbucket)
The most significant architectural challenge: ADO is a monolithic platform where Work Items, VCS, and Pipelines share a single API surface. In the Atlassian ecosystem, these are typically **two separate products**:
- **Jira** for issue tracking (separate URL, auth, config)
- **Bitbucket** for VCS and Pipelines

The `Config` struct and provider factory must accommodate a "compound provider" where `IssueTracker` is backed by Jira and `VCSProvider` is backed by Bitbucket, each with independent credentials and URLs.

### 2. Jira Issue Key Format
ADO Work Items and GitHub Issues use plain integers for IDs. Jira uses alphanumeric keys (`PROJ-123`). The FM branch naming convention `feature/{id}-slug` would need to accommodate keys like `feature/PROJ-123-slug`. The `parse_wi_id_from_branch` function in `src/core/` must be updated to handle both numeric IDs and `[A-Z]+-\d+` patterns.

### 3. Jira State Transitions
Unlike direct state assignment (`update_work_item_state`), Jira requires a two-step process:
1.  `GET /transitions` to fetch available transitions from the current state.
2.  `POST /transitions` with the chosen transition ID.

The `IssueTracker` trait's `update_work_item_state(id, state)` signature implies direct assignment. The GitLab/GitHub providers can use this, but Jira needs the transition lookup step hidden inside the implementation.

### 4. Jira AccountId vs. Email
ADO and most providers use email addresses to identify users. Jira Cloud's v3 API uses opaque `accountId` UUIDs. The FM configuration and `add_reviewer` logic must handle a lookup step (email → accountId) or store accountIds directly.

### 5. Bitbucket Workspace Identifier
Bitbucket Cloud uses a `workspace` + `repo_slug` pair to identify repositories (e.g., `mycompany/my-repo`). This is analogous to GitHub's `owner/repo` and different from ADO's `organization/project/repo` triple.

### 6. No Draft PR Support
Bitbucket Cloud lacks a native Draft PR feature. If FM relies on draft PRs as part of the activity lifecycle (e.g., `fm work new` creates a draft PR), the Bitbucket provider must either skip this step or emulate it with a `[WIP]` prefix convention.

### 7. Bitbucket Server / Data Center
Enterprises may use Bitbucket Server (self-hosted) or Bitbucket Data Center. The API differs from Bitbucket Cloud in several ways. A separate implementation or strong abstraction would be needed. Scope for an initial implementation should target Bitbucket Cloud only.

### 8. Smart Commits and Jira Integration
The Jira–Bitbucket Smart Commits feature (e.g., `git commit -m "PROJ-123 #in-progress Fixed bug"`) provides a native mechanism that partially overlaps with FM's `create_artifact_link`. The provider can leverage this pattern by ensuring branch names and commit messages follow the Smart Commit format.

## 4. Suggested Refactorings

1.  **Compound Provider Support:**
    The provider factory must support composing an `IssueTracker` (Jira) with a `VCSProvider` (Bitbucket) independently. Introduce a `ProviderSet` struct that holds separate trait objects for each concern.

2.  **Generic WI ID Type:**
    The `WorkItemId` type should be a `String` (or a newtype wrapping `String`) rather than an integer to accommodate Jira's `PROJ-123` format. The `parse_wi_id_from_branch` function needs a regex that handles both `\d+` and `[A-Z]+-\d+`.

3.  **State Transition Abstraction:**
    The `update_work_item_state` method signature can remain as-is, but the Jira implementation must internally perform the transition lookup. Add an optional `get_available_transitions(id)` method to the trait for providers that need it.

4.  **Search Abstraction:**
    Replace `query_work_items(wiql: &str)` with `query_work_items(filter: WorkItemFilter)`. For Jira, the provider builds a JQL string from the struct internally.

5.  **User Identity Abstraction:**
    Introduce a `UserId` enum (`Email(String)` | `AccountId(String)`) to handle both ADO/GitLab email-based identification and Jira's UUID-based system.

6.  **Draft PR Fallback:**
    Add a `SupportedFeatures` capability flags struct per provider so FM can gracefully degrade when a provider doesn't support a feature (e.g., draft PRs).

## 5. Conclusion

Implementing a Bitbucket/Atlassian provider is **feasible but architecturally more complex** than GitHub or GitLab due to:

1.  The split between Jira (issue tracking) and Bitbucket (VCS/Pipelines) requiring a compound provider approach in the `Config` and factory layer.
2.  Jira's alphanumeric issue keys requiring changes to the branch naming parser and potentially the FM branch convention.
3.  Jira's transition-based state model requiring the provider to hide multi-step state changes.
4.  The absence of Draft PR support in Bitbucket Cloud requiring graceful degradation.

The Bitbucket VCS and Pipeline API surface itself is straightforward and maps cleanly to the existing traits. The primary investment is in the Jira issue tracker integration and the compound provider architecture. Targeting **Bitbucket Cloud + Jira Cloud** with the v2/v3 REST APIs is recommended as the first milestone, deferring Bitbucket Server/Data Center and Bamboo support.
