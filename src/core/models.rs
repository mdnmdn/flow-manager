use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct WorkItemId(pub String);

impl WorkItemId {
    pub fn from_int(n: i64) -> Self {
        WorkItemId(n.to_string())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for WorkItemId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<i64> for WorkItemId {
    fn from(n: i64) -> Self {
        WorkItemId(n.to_string())
    }
}

impl From<&str> for WorkItemId {
    fn from(s: &str) -> Self {
        WorkItemId(s.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkItem {
    pub id: WorkItemId,
    pub title: String,
    pub work_item_type: String,
    pub state: String,
    pub description: Option<String>,
    pub assigned_to: Option<String>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityIssue {
    pub key: String,
    pub message: String,
    pub severity: String,
    pub component: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    pub id: String,
    pub title: String,
    pub status: String,
    pub source_branch: String,
    pub target_branch: String,
    pub is_draft: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pipeline {
    pub id: String,
    pub name: String,
    pub folder: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineRun {
    pub id: String,
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

#[derive(Debug, Default, Clone)]
pub struct WorkItemFilter {
    pub state: Option<String>,
    pub assigned_to_me: bool,
    pub work_item_type: Option<String>,
    pub text: Option<String>,
    pub milestone: Option<String>,
    pub limit: Option<u32>,
}
