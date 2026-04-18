use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkItem {
    pub id: i32,
    pub title: String,
    pub work_item_type: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    pub id: i32,
    pub title: String,
    pub status: String,
    pub source_branch: String,
    pub target_branch: String,
    pub is_draft: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pipeline {
    pub id: i32,
    pub name: String,
    pub folder: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRun {
    pub id: i32,
    pub status: String,
    pub result: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repository {
    pub id: String,
    pub name: String,
    pub project_id: String,
    pub default_branch: Option<String>,
}
