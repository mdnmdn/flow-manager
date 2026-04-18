use async_trait::async_trait;
use crate::core::models::{WorkItem, PullRequest, Pipeline, PipelineRun, Repository};
use anyhow::Result;

#[async_trait]
pub trait IssueTracker {
    async fn get_work_item(&self, id: i32) -> Result<WorkItem>;
    async fn create_work_item(&self, title: &str, work_item_type: &str) -> Result<WorkItem>;
    async fn update_work_item(&self, id: i32, state: &str) -> Result<WorkItem>;
}

#[async_trait]
pub trait VCSProvider {
    async fn get_pull_request(&self, repository: &str, id: i32) -> Result<PullRequest>;
    async fn create_pull_request(
        &self,
        repository: &str,
        title: &str,
        source: &str,
        target: &str,
    ) -> Result<PullRequest>;
    async fn create_branch(&self, repository: &str, name: &str, source: &str) -> Result<()>;
    async fn get_repository(&self, name: &str) -> Result<Repository>;
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
