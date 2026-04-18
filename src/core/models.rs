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
