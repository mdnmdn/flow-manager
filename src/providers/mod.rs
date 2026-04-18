use crate::core::models::{
    MergeStrategy, Pipeline, PipelineRun, PullRequest, Repository, WorkItem,
};
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait IssueTracker {
    async fn get_work_item(&self, id: i32) -> Result<WorkItem>;
    async fn create_work_item(&self, title: &str, work_item_type: &str) -> Result<WorkItem>;
    async fn update_work_item(&self, id: i32, state: &str) -> Result<WorkItem>;
}

#[async_trait]
pub trait VCSProvider {
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
    async fn create_branch(&self, repository: &str, name: &str, source: &str) -> Result<()>;
    async fn delete_branch(&self, repository: &str, name: &str) -> Result<()>;
    async fn get_repository(&self, name: &str) -> Result<Repository>;
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
