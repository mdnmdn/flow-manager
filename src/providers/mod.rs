use crate::core::models::{
    MergeStrategy, Pipeline, PipelineRun, PullRequest, QualityIssue, Repository, WorkItem,
    WorkItemFilter, WorkItemId,
};
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait IssueTracker {
    async fn get_work_item(&self, id: &WorkItemId) -> Result<WorkItem>;
    async fn create_work_item(
        &self,
        title: &str,
        work_item_type: &str,
        description: Option<&str>,
        assigned_to: Option<&str>,
        tags: Option<Vec<&str>>,
    ) -> Result<WorkItem>;
    async fn update_work_item(
        &self,
        id: &WorkItemId,
        title: Option<&str>,
        description: Option<&str>,
        assigned_to: Option<&str>,
        tags: Option<Vec<&str>>,
    ) -> Result<WorkItem>;
    async fn update_work_item_state(&self, id: &WorkItemId, state: &str) -> Result<WorkItem>;
    async fn query_work_items(&self, filter: &WorkItemFilter) -> Result<Vec<WorkItem>>;
    async fn create_artifact_link(&self, wi_id: &WorkItemId, url: &str) -> Result<()>;
    async fn link_work_items(
        &self,
        source_id: &WorkItemId,
        target_id: &WorkItemId,
        relation: &str,
    ) -> Result<()>;
    async fn get_child_work_items(
        &self,
        id: &WorkItemId,
        work_item_type: Option<&str>,
    ) -> Result<Vec<WorkItem>>;
    async fn available_states(&self, _id: &WorkItemId) -> Result<Vec<String>> {
        Ok(vec![])
    }
}

#[async_trait]
pub trait VCSProvider {
    // Remote Pull Request Management
    async fn get_pull_request_by_branch(
        &self,
        repository: &str,
        branch: &str,
    ) -> Result<Option<PullRequest>>;
    async fn get_pull_request_details(&self, repository: &str, id: &str) -> Result<PullRequest>;
    async fn create_pull_request(
        &self,
        repository: &str,
        source: &str,
        target: &str,
        title: &str,
        description: &str,
        is_draft: bool,
    ) -> Result<PullRequest>;
    async fn update_pull_request(
        &self,
        repository: &str,
        id: &str,
        title: Option<&str>,
        description: Option<&str>,
        is_draft: Option<bool>,
        status: Option<&str>,
    ) -> Result<PullRequest>;
    async fn complete_pull_request(
        &self,
        repository: &str,
        id: &str,
        strategy: MergeStrategy,
        delete_source_branch: bool,
    ) -> Result<()>;
    async fn add_reviewer(&self, repository: &str, id: &str, reviewer_id: &str) -> Result<()>;

    // Remote Branch/Repo Management
    async fn create_branch(&self, repository: &str, name: &str, source: &str) -> Result<()>;
    async fn delete_branch(&self, repository: &str, name: &str) -> Result<()>;
    async fn get_repository(&self, name: &str) -> Result<Repository>;

    // Local Git Operations
    async fn get_current_branch(&self) -> Result<String>;
    async fn checkout_branch(&self, name: &str) -> Result<()>;
    async fn get_status(&self) -> Result<String>;
    async fn stash_push(&self, message: &str) -> Result<()>;
    async fn stash_pop(&self) -> Result<()>;
    async fn push(&self, force: bool) -> Result<()>;
    async fn pull(&self) -> Result<()>;
    async fn fetch(&self) -> Result<()>;
    async fn commit(&self, message: &str, all: bool, amend: bool) -> Result<()>;
    async fn discard_local_changes(&self) -> Result<()>;
    async fn get_log(&self, range: Option<&str>, limit: Option<i32>) -> Result<String>;
    async fn merge(&self, source: &str) -> Result<()>;
    async fn rebase(&self, target: &str) -> Result<()>;

    // Submodule Support
    async fn check_submodule_status(&self, path: &str) -> Result<bool>; // returns true if ahead/changed
    async fn update_submodule_pointer(&self, path: &str) -> Result<()>;
}

#[async_trait]
pub trait PipelineProvider {
    async fn list_pipelines(&self) -> Result<Vec<Pipeline>>;
    async fn run_pipeline(&self, pipeline_id: &str, branch: &str) -> Result<PipelineRun>;
    async fn get_latest_run(&self, branch: &str) -> Result<Option<PipelineRun>>;
    async fn get_run_status(&self, run_id: &str) -> Result<PipelineRun>;
}

#[async_trait]
pub trait QualityProvider {
    async fn get_open_issues(
        &self,
        project_key: &str,
        severity: Option<&str>,
    ) -> Result<Vec<QualityIssue>>;
}

#[derive(Debug, Default, Clone)]
pub struct ProviderCapabilities {
    pub draft_pull_requests: bool,
    pub pipeline_support: bool,
    pub work_item_hierarchy: bool,
    pub formal_artifact_links: bool,
    pub merge_strategies: Vec<MergeStrategy>,
    pub work_item_relations: Vec<String>,
}

pub trait CapableProvider {
    fn capabilities(&self) -> ProviderCapabilities;
}

pub struct ProviderSet {
    pub issue_tracker: Box<dyn IssueTracker + Send + Sync>,
    pub vcs: Box<dyn VCSProvider + Send + Sync>,
    pub pipeline: Option<Box<dyn PipelineProvider + Send + Sync>>,
    pub quality: Option<Box<dyn QualityProvider + Send + Sync>>,
}

pub mod adonet;
pub mod git;
pub mod sonar;
