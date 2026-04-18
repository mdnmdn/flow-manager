use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkItem {
    pub id: i32,
    pub title: String,
    pub work_item_type: String,
    pub state: String,
    pub description: Option<String>,
    pub assigned_to: Option<String>,
    pub tags: Vec<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MergeStrategy {
    Squash,
    Rebase,
    RebaseMerge,
    NoFastForward,
}

impl std::fmt::Display for MergeStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            MergeStrategy::Squash => "squash",
            MergeStrategy::Rebase => "rebase",
            MergeStrategy::RebaseMerge => "rebaseMerge",
            MergeStrategy::NoFastForward => "noFastForward",
        };
        write!(f, "{}", s)
    }
}
