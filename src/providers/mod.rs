use crate::core::models::{
    MergeStrategy, Pipeline, PipelineRun, PullRequest, Repository, WorkItem,
};
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait IssueTracker {
    async fn get_work_item(&self, id: i32) -> Result<WorkItem>;
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
        id: i32,
        title: Option<&str>,
        description: Option<&str>,
        assigned_to: Option<&str>,
        tags: Option<Vec<&str>>,
    ) -> Result<WorkItem>;
    async fn update_work_item_state(&self, id: i32, state: &str) -> Result<WorkItem>;
    async fn query_work_items(&self, wiql: &str) -> Result<Vec<WorkItem>>;
    async fn create_artifact_link(&self, wi_id: i32, url: &str) -> Result<()>;
    async fn link_work_items(&self, source_id: i32, target_id: i32, relation: &str) -> Result<()>;
    async fn get_child_work_items(
        &self,
        id: i32,
        work_item_type: Option<&str>,
    ) -> Result<Vec<WorkItem>>;
}

#[async_trait]
pub trait VCSProvider {
    // Remote Pull Request Management
    async fn get_pull_request_by_branch(
        &self,
        repository: &str,
        branch: &str,
    ) -> Result<Option<PullRequest>>;
    async fn get_pull_request_details(&self, repository: &str, id: i32) -> Result<PullRequest>;
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
        id: i32,
        title: Option<&str>,
        description: Option<&str>,
        is_draft: Option<bool>,
        status: Option<&str>,
    ) -> Result<PullRequest>;
    async fn complete_pull_request(
        &self,
        repository: &str,
        id: i32,
        strategy: MergeStrategy,
        delete_source_branch: bool,
    ) -> Result<()>;
    async fn add_reviewer(&self, repository: &str, id: i32, reviewer_id: &str) -> Result<()>;

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
    async fn commit(&self, message: &str, all: bool) -> Result<()>;

    // Submodule Support
    async fn check_submodule_status(&self, path: &str) -> Result<bool>; // returns true if ahead/changed
    async fn update_submodule_pointer(&self, path: &str) -> Result<()>;
}

#[async_trait]
pub trait PipelineProvider {
    async fn list_pipelines(&self) -> Result<Vec<Pipeline>>;
    async fn run_pipeline(&self, pipeline_id: i32, branch: &str) -> Result<PipelineRun>;
    async fn get_pipeline_run(&self, pipeline_id: i32, run_id: i32) -> Result<PipelineRun>;
}

pub mod adonet;
pub mod git;
pub mod sonar;
