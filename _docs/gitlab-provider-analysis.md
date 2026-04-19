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

### `IssueTracker` Trait

*   `query_work_items(wiql: &str)`: **Critical.** WIQL is entirely ADO-specific. GitLab uses a REST API with query parameters (e.g., `state`, `labels`, `milestone`, `assignee_username`) or a GraphQL API for more complex queries.
    *   *Suggestion:* Replace with a structured `WorkItemFilter` that each provider translates to its native API call. GitLab's filter model maps very naturally to a struct-based approach.
*   `create_artifact_link(wi_id, url)`: GitLab supports formal "Related Issues" links and auto-close keywords in MR descriptions (`Closes #123`). The provider can implement this by updating the issue description or creating a formal issue link via `/projects/:id/issues/:issue_iid/links`.
*   `link_work_items(source_id, target_id, relation)`: Maps well to GitLab's Issue Links API (`/issues/:issue_iid/links`), which supports `relates_to`, `blocks`, and `is_blocked_by` relation types — more formal than GitHub, less rigid than ADO.
*   `get_child_work_items(id, type)`: Maps to GitLab's native child issue support. On free tiers, child issues are managed via the UI but not exposed as a dedicated flat API endpoint; on Premium/Ultimate they are accessible as task relationships.
*   `update_work_item_state(id, state)`: Maps to the issue state transitions (`opened`/`closed`). Custom workflow states require Premium.

### `VCSProvider` Trait

The `VCSProvider` trait maps well to GitLab's REST and GraphQL APIs:

*   **PR Management (MR Management):** `create_pull_request`, `update_pull_request`, `complete_pull_request` map directly to GitLab's `/projects/:id/merge_requests` endpoints. The terminology difference (PR → MR) is internal to the provider implementation.
*   **Merge Strategies:** `MergeStrategy` maps to GitLab's merge methods: `merge` (standard), `squash` (squash commit), and `rebase_merge`. All three are natively supported by GitLab.
*   **Draft PRs:** GitLab supports Draft MRs using the `Draft:` title prefix, which maps to the `is_draft` flag.
*   **Reviewers:** GitLab uses `reviewer_ids` on Merge Requests (available on all tiers since GitLab 13.8).
*   **Delete Source Branch:** Natively supported via `should_remove_source_branch` on MR completion.

### `PipelineProvider` Trait

GitLab CI/CD provides a comprehensive API:

*   `list_pipelines`: Maps to `/projects/:id/pipelines` (list pipeline runs) or for pipeline *definitions*, inspecting `.gitlab-ci.yml` for job/stage names. A single project has one pipeline definition (the YAML), making the concept of "listing pipeline definitions" less relevant than in ADO.
*   `run_pipeline`: Maps to `/projects/:id/pipeline` (POST to trigger) or via pipeline triggers and the Trigger API.
*   `get_pipeline_run`: Maps to `/projects/:id/pipelines/:pipeline_id`.
*   `get_run_status`: Pipeline status values (`created`, `waiting_for_resource`, `preparing`, `pending`, `running`, `success`, `failed`, `canceled`, `skipped`, `manual`, `scheduled`) are richer than ADO's and need mapping to FM's internal status model.

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
ADO Pipelines have distinct named "Pipeline Definitions" with integer IDs. In GitLab, a project has a single `.gitlab-ci.yml` that defines all jobs/stages. The `list_pipelines()` abstraction could return recent pipeline runs or a synthetic list of named CI/CD jobs instead.

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
