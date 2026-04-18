# GitHub Provider Analysis for Flow Manager (`fm`)

This document analyzes the feasibility of implementing a GitHub provider for the Flow Manager (`fm`) tool, evaluating the alignment with current project structure, traits, and domain models.

## 1. Concept Mapping

| FM Domain / ADO Concept | GitHub Equivalent | Notes |
|-------------------------|-------------------|-------|
| **Work Item (WI)**      | **Issue**         | GitHub Issues represent the unit of work. |
| **User Story / Bug**    | **Issue + Labels**| Differentiated via labels (e.g., `feature`, `bug`). |
| **WI ID**               | **Issue Number**  | Sequential integers within a repository. |
| **WI State**            | **Issue State**   | GitHub has `open`/`closed`, but Projects (v2) support custom statuses. |
| **Pull Request (PR)**   | **Pull Request**  | Direct 1:1 mapping. |
| **Pipeline**            | **Workflow**      | GitHub Actions workflows. |
| **Pipeline Run**        | **Workflow Run**  | Direct 1:1 mapping. |
| **Artifact Links**      | **Mentions / Keywords** | GitHub uses "fixes #123" or mentions in descriptions to link items. |
| **Child Work Items**    | **Task Lists / Sub-issues** | GitHub uses Markdown task lists or the newer Sub-issues (beta). |

## 2. Trait Compatibility Analysis

### `IssueTracker` Trait
The current trait has some ADO-specific "leaks":

*   `query_work_items(wiql: &str)`: **Critical.** `wiql` is a SQL-like language specific to Azure DevOps. GitHub uses a completely different search syntax (e.g., `is:issue is:open label:feature`).
    *   *Suggestion:* Introduce a `WorkItemFilter` struct or use a more generic search string that the provider translates.
*   `create_artifact_link(wi_id, url)`: In ADO, this creates a formal link to a Git ref or PR. In GitHub, this is typically done by appending a comment or updating the issue description with the URL/Reference.
*   `link_work_items(source_id, target_id, relation)`: Maps well to GitHub's issue linking/mentions, though "relations" are less formal than ADO's (Parent/Child, Related).
*   `get_child_work_items(id, type)`: Maps to Task Lists in the issue body or sub-issues. Parsing might be required if using Task Lists.

### `VCSProvider` Trait
The `VCSProvider` trait is mostly generic and maps well to GitHub's REST API:

*   **PR Management:** `create_pull_request`, `update_pull_request`, `complete_pull_request` (merge) map directly to GitHub's `/repos/{owner}/{repo}/pulls` endpoints.
*   **Merge Strategies:** `MergeStrategy` (Squash, Rebase, RebaseMerge) maps to GitHub's merge methods.
*   **Reviewers:** GitHub uses `requested_reviewers`.

### `PipelineProvider` Trait
*   `list_pipelines`: Maps to `/repos/{owner}/{repo}/actions/workflows`.
*   `run_pipeline`: Maps to `workflow_dispatch` events.
*   `get_pipeline_run`: Maps to `/repos/{owner}/{repo}/actions/runs/{run_id}`.

## 3. Critical Points & Challenges

### 1. WIQL Dependency
Commands like `fm work list` currently expect the provider to handle a WIQL string. If we add GitHub, the command layer must either:
- Know which provider is active and send the correct syntax (Bad: leaks provider details).
- Send a neutral filter object that the provider translates (Good).

### 2. Artifact Linking (Activity Invariants)
The "Activity Invariants" described in `porcelain-commands-proposal.md` rely on ADO artifact links.
- GitHub doesn't have an exact "Artifact Link" API.
- *Feasibility:* The GitHub provider can implement `create_artifact_link` by updating the Issue description or adding a standardized comment (e.g., "Branch: feature/123-slug").

### 3. Work Item Hierarchy
ADO has a strict Parent/Child hierarchy. GitHub uses:
1.  **Markdown Task Lists:** `- [ ] #123`.
2.  **Sub-issues (New):** Formal hierarchy.
*   *Challenge:* The implementation of `get_child_work_items` in a GitHub provider will need to decide which mechanism to support.

### 4. Configuration Structure
The current `Config` struct in `src/core/config.rs` has an `ado` field.
```rust
pub struct Config {
    pub ado: AdoConfig,
    pub sonar: Option<SonarConfig>,
    pub fm: FmConfig,
}
```
This needs to be refactored to support either `ado` OR `github` (or a generic `provider` config).

## 4. Suggested Refactorings

1.  **Generic Provider Configuration:**
    Use an enum or optional blocks for different providers in `Config`.
2.  **Search Abstraction:**
    Change `query_work_items(query: &str)` to accept a structured `Query` or a more provider-neutral format.
3.  **Provider Factory:**
    The CLI currently seems to be evolving towards using `AzureDevOpsProvider` directly or via the traits. A factory or a dynamic dispatch mechanism will be needed to select the provider based on configuration.
4.  **Context Discovery:**
    `fm context` derives the WI ID from the branch name. This is provider-agnostic and should continue to work. However, the "Repository" identifier in GitHub is `owner/repo`, while in ADO it's often just a name within a project.

## 5. Conclusion

Implementing a GitHub provider is **highly feasible** but requires some "de-ADO-ification" of the `IssueTracker` trait and the `Config` model. The most significant effort lies in:
1.  Refactoring the search logic to move away from raw WIQL.
2.  Implementing the "Artifact Link" logic using GitHub mentions/comments to maintain activity invariants.
3.  Updating the configuration loading to handle GitHub credentials (PAT/OAuth) and repository paths.
