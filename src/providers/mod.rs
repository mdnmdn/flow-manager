use async_trait::async_trait;
use crate::core::models::{WorkItem, PullRequest};
use anyhow::Result;

#[async_trait]
pub trait IssueTracker {
    async fn get_work_item(&self, id: i32) -> Result<WorkItem>;
    async fn create_work_item(&self, title: &str, work_item_type: &str) -> Result<WorkItem>;
    async fn update_work_item(&self, id: i32, state: &str) -> Result<WorkItem>;
}

#[async_trait]
pub trait VCSProvider {
    async fn get_pull_request(&self, id: i32) -> Result<PullRequest>;
    async fn create_pull_request(&self, title: &str, source: &str, target: &str) -> Result<PullRequest>;
    async fn create_branch(&self, name: &str, source: &str) -> Result<()>;
}
