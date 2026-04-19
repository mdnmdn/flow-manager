# GitLab Provider Analysis for Flow Manager (`fm`)

This document analyzes the feasibility of implementing a GitLab provider for the Flow Manager (`fm`) tool, evaluating the alignment with current project structure, traits, and domain models.

## 1. Concept Mapping

| FM Domain / ADO Concept | GitLab Equivalent | Notes |
|-------------------------|-------------------|-------|
| **Work Item (WI)**      | **Issue**         | GitLab Issues are the primary unit of work. |
| **User Story / Bug**    | **Issue + Labels / Issue Types** | Differentiated via labels or Issue Types (Premium). |
| **WI ID**               | **Issue IID**     | Per-project sequential integers (internal ID). Note: GitLab also has a global numeric ID. |
| **WI State**            | **Issue State**   | GitLab has `opened`/`closed`, plus custom states via workflows (Premium). |
| **WI Tags**             | **Labels**        | GitLab has a rich hierarchical label system (group + project levels). |
| **WI Assigned To**      | **Assignees**     | GitLab supports multiple assignees per issue. |
| **Pull Request (PR)**   | **Merge Request (MR)** | Direct conceptual mapping. GitLab terminology differs. |
| **Pipeline**            | **CI/CD Pipeline** | GitLab CI/CD defined in `.gitlab-ci.yml`. |
| **Pipeline Run**        | **Pipeline**      | Each triggered `.gitlab-ci.yml` execution is a "Pipeline" in GitLab. |
| **Artifact Links**      | **Issue Links / MR References** | GitLab supports formal issue links and auto-closing via MR descriptions. |
| **Child Work Items**    | **Child Issues / Task Lists** | GitLab supports parent/child issue relationships natively (via "Child Issues"). |
| **Area / Iteration**    | **Epic / Milestone / Iteration** | GitLab Epics (Ultimate) group issues; Milestones are available in all tiers. |

## 2. Trait Compatibility Analysis

Actual trait signatures (from `src/providers/mod.rs`) and domain models (from `src/core/models.rs`) are referenced throughout this section.

### `IssueTracker` Trait

*   `get_work_item(id: i32)`, `update_work_item(id: i32, ...)`, `create_artifact_link(wi_id: i32, url: &str)`, `link_work_items(source_id: i32, target_id: i32, ...)`, `get_child_work_items(id: i32, ...)`: All use `i32` for the issue ID. GitLab issue **IIDs** (the per-project sequential integers exposed in the UI and API paths) are integers — **fully compatible** with the current `i32` type. Note: GitLab also has a global numeric `id` distinct from the project-local `iid`; the provider must consistently use `iid`.
*   `query_work_items(wiql: &str)`: **Critical.** WIQL is entirely ADO-specific. GitLab uses REST query parameters (e.g., `state`, `labels`, `milestone`, `assignee_username`).
    *   *Suggestion:* Replace with a structured `WorkItemFilter` that each provider translates to its native API call. GitLab's filter model maps very naturally to a struct-based approach.
*   `create_artifact_link(wi_id: i32, url: &str)`: GitLab supports formal "Related Issues" links and auto-close keywords in MR descriptions (`Closes #123`). The provider can implement this via `/projects/:id/issues/:issue_iid/links` or by appending closing keywords to the MR description.
*   `link_work_items(source_id: i32, target_id: i32, relation: &str)`: Maps to GitLab's Issue Links API (`/issues/:issue_iid/links`), which supports `relates_to`, `blocks`, and `is_blocked_by` relation types — more formal than GitHub, less rigid than ADO.
*   `get_child_work_items(id: i32, type)`: GitLab supports parent/child issue relationships. On free tiers the API support is limited; Premium/Ultimate exposes them as formal task relationships.
*   `update_work_item_state(id: i32, state: &str)`: Maps to the `state_event` parameter (`close`/`reopen`) on the Issues API. State transitions in GitLab are direct — no transition-ID lookup step is needed. Custom workflow states require Premium.

### `VCSProvider` Trait

The `VCSProvider` trait maps well to GitLab's REST API:

*   **PR Management (MR Management):** `create_pull_request`, `update_pull_request`, `complete_pull_request` map directly to GitLab's `/projects/:id/merge_requests` endpoints. The terminology difference (PR → MR) is fully internal to the provider.
*   **Merge Strategies:** FM's `MergeStrategy` enum has four variants; GitLab exposes three merge methods. The mapping is:
    | FM `MergeStrategy`  | GitLab method         |
    |---------------------|-----------------------|
    | `NoFastForward`     | Merge commit          |
    | `Squash`            | Squash and merge      |
    | `RebaseMerge`       | Rebase and merge      |
    | `Rebase`            | *(no direct equivalent)* |
    The `Rebase` variant (pure rebase, no merge commit) has no standard GitLab merge method. GitLab does have a `/rebase` quick action but it does not complete the MR. The provider should either map `Rebase` to `RebaseMerge` or reject it with an explicit error.
*   **Draft PRs:** GitLab supports Draft MRs with a dedicated `draft` boolean field in the API response. Setting `draft: true` on creation maps directly to `is_draft`.
*   **Reviewers:** GitLab uses `reviewer_ids` (numeric user IDs) on MRs, available on all tiers since GitLab 13.8. The `add_reviewer(repository, id, reviewer_id: &str)` signature accepts a string, so numeric IDs stored as strings work fine.
*   **Delete Source Branch:** Natively supported via `should_remove_source_branch` on MR completion.

### `PipelineProvider` Trait

GitLab CI/CD provides a comprehensive API, but the pipeline abstraction requires care:

*   `list_pipelines() -> Vec<Pipeline>`: The `Pipeline` struct has `{ id: i32, name: String, folder: String }`. GitLab has no named pipeline definitions — a project has one `.gitlab-ci.yml`. The provider can return the most recent pipeline runs as synthetic "definitions" (using the run ID and branch/ref as name), but this is a semantic mismatch with ADO. A synthetic single entry representing the project's CI configuration may be more honest.
*   `run_pipeline(pipeline_id: i32, branch: &str)`: GitLab triggers a new pipeline via `POST /projects/:id/pipeline` with `{ ref: branch }`. The `pipeline_id` argument has no equivalent — triggering does not reference a prior run. The provider should ignore `pipeline_id` (or use a sentinel value `0`) and trigger on the branch alone.
*   `get_latest_run(branch: &str) -> Option<PipelineRun>`: Maps to `GET /projects/:id/pipelines?ref=branch&order_by=id&sort=desc&per_page=1`.
*   `get_run_status(run_id: i32) -> PipelineRun`: Maps to `GET /projects/:id/pipelines/:pipeline_id`. GitLab pipeline IDs are integers — **compatible** with `i32`.
*   Pipeline status values (`created`, `waiting_for_resource`, `pending`, `running`, `success`, `failed`, `canceled`, `skipped`, `manual`, `scheduled`) are richer than ADO's and must be mapped to FM's internal `PipelineRun.status: String` field.

## 3. Critical Points & Challenges

### 1. WIQL Dependency
Same as the GitHub analysis. The `query_work_items(wiql: &str)` signature is incompatible. The command layer must be refactored to use a provider-neutral `WorkItemFilter`.

### 2. Terminology: MR vs. PR
GitLab calls them "Merge Requests" rather than "Pull Requests." This is purely a naming difference — the FM domain model can use "PR" internally and the GitLab provider maps to MR endpoints.

### 3. Project Identification
GitLab identifies repositories via `namespace/project` slugs or numeric project IDs. The `Config` struct's `ado` block must be replaced with a provider-agnostic block that includes the `project_id` or `namespace/project` path for GitLab.

### 4. Auto-close Keywords and Artifact Linking
GitLab natively supports closing issues via MR descriptions (`Closes #123`, `Fixes #456`). The `create_artifact_link` can leverage this by appending closing keywords to the MR description, maintaining the Activity Invariants without a separate API call.

### 5. Authentication
GitLab uses Personal Access Tokens (PAT), OAuth 2.0, or Job tokens (for CI). The token must have `api` scope. This aligns well with the current PAT-based ADO authentication pattern in FM.

### 6. Self-Hosted vs. GitLab.com
Many enterprises run self-hosted GitLab instances. The configuration must support a custom `base_url` rather than hardcoding `https://gitlab.com`. This is similar to the ADO organization URL pattern already present.

### 7. Pipeline Model Difference
ADO Pipelines have distinct named "Pipeline Definitions" with integer IDs. In GitLab, a project has a single `.gitlab-ci.yml` that defines all jobs/stages. The `run_pipeline(pipeline_id: i32, branch: &str)` signature takes a definition ID that has no GitLab equivalent — the provider must treat `pipeline_id` as irrelevant and always trigger by branch. The `list_pipelines()` abstraction should return a synthetic single entry or recent run history rather than definitions.

### 8. `MergeStrategy::Rebase` Gap
The FM `MergeStrategy` enum includes a `Rebase` variant with no direct GitLab merge method equivalent. The GitLab provider must document and handle this case — either mapping it to `RebaseMerge` or returning an unsupported error.

## 4. Suggested Refactorings

1.  **Generic Provider Configuration:**
    Introduce a `provider` enum in `Config` (e.g., `Provider::Ado`, `Provider::GitHub`, `Provider::GitLab`) with provider-specific subconfigs. This supersedes the current `ado`-only field.

2.  **Search Abstraction:**
    Replace `query_work_items(wiql: &str)` with `query_work_items(filter: WorkItemFilter)`. GitLab's API parameters (`state`, `labels`, `assignee`, `milestone`) map cleanly to struct fields.

3.  **PR/MR Terminology Abstraction:**
    Keep the FM domain using "PR" internally. The GitLab provider implementation uses MR endpoints transparently.

4.  **Pipeline Abstraction:**
    Define a `PipelineDefinition` struct with a name and opaque ID. The GitLab provider can expose named CI/CD pipelines or stages as definitions.

5.  **Provider Factory:**
    A factory or registry pattern in `src/providers/` to instantiate the correct provider based on the active configuration.

## 5. Conclusion

Implementing a GitLab provider is **highly feasible** and arguably slightly easier than GitHub due to GitLab's more structured API (formal issue links, native child issues, explicit MR reviewer support). The main effort lies in:

1.  Replacing WIQL with a structured `WorkItemFilter` (shared with GitHub work).
2.  Mapping the GitLab pipeline model (single YAML, multiple runs) to FM's pipeline abstraction.
3.  Updating `Config` to support multi-provider configuration with optional `base_url` for self-hosted instances.
4.  Ensuring the MR terminology difference is fully contained within the GitLab provider implementation.
