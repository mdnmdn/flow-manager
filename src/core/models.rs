use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorkItemId(pub String);

impl WorkItemId {
    pub fn from_int(n: i32) -> Self {
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

impl From<i32> for WorkItemId {
    fn from(id: i32) -> Self {
        WorkItemId::from_int(id)
    }
}

impl From<String> for WorkItemId {
    fn from(id: String) -> Self {
        WorkItemId(id)
    }
}

impl From<&str> for WorkItemId {
    fn from(id: &str) -> Self {
        WorkItemId(id.to_string())
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

#[derive(Debug, Default)]
pub struct WorkItemFilter {
    pub state: Option<String>,
    pub assigned_to: Option<String>,
    pub labels: Vec<String>,
    pub work_item_type: Option<String>,
    pub text: Option<String>,
    pub milestone: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UserId {
    Email(String),
    AccountId(String), // Jira Cloud UUID
    Username(String),  // GitHub login, GitLab username
}

impl std::fmt::Display for UserId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UserId::Email(e) => write!(f, "{}", e),
            UserId::AccountId(a) => write!(f, "{}", a),
            UserId::Username(u) => write!(f, "{}", u),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderCapabilities {
    pub draft_pull_requests: bool,
    pub pipeline_support: bool,
    pub work_item_hierarchy: bool,   // parent/child relationships
    pub formal_artifact_links: bool, // vs. description/comment-based linking
    pub merge_strategies: Vec<MergeStrategy>,
    pub work_item_relations: Vec<String>, // "blocks", "relates_to", "parent", etc.
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
