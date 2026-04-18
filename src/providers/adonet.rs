use async_trait::async_trait;
use crate::providers::{IssueTracker, VCSProvider};
use crate::core::models::{WorkItem, PullRequest};
use anyhow::Result;

use crate::core::config::AdoConfig;

pub struct AzureDevOpsProvider {
    pub organization_url: String,
    pub project: String,
    pub pat: String,
}

impl AzureDevOpsProvider {
    pub fn new(config: &AdoConfig) -> Self {
        Self {
            organization_url: config.url.clone(),
            project: config.project.clone(),
            pat: config.pat.clone(),
        }
    }
}

#[async_trait]
impl IssueTracker for AzureDevOpsProvider {
    async fn get_work_item(&self, _id: i32) -> Result<WorkItem> {
        todo!("Implement ADO get_work_item")
    }
    async fn create_work_item(&self, _title: &str, _work_item_type: &str) -> Result<WorkItem> {
        todo!("Implement ADO create_work_item")
    }
    async fn update_work_item(&self, _id: i32, _state: &str) -> Result<WorkItem> {
        todo!("Implement ADO update_work_item")
    }
}

#[async_trait]
impl VCSProvider for AzureDevOpsProvider {
    async fn get_pull_request(&self, _id: i32) -> Result<PullRequest> {
        todo!("Implement ADO get_pull_request")
    }
    async fn create_pull_request(&self, _title: &str, _source: &str, _target: &str) -> Result<PullRequest> {
        todo!("Implement ADO create_pull_request")
    }
    async fn create_branch(&self, _name: &str, _source: &str) -> Result<()> {
        todo!("Implement ADO create_branch")
    }
}
