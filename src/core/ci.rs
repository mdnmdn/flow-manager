use crate::core::config::CiConfig;

#[derive(Debug, Clone)]
pub enum CiPlatform {
    AzureDevOps,
}

impl CiPlatform {
    pub fn label(&self) -> &'static str {
        match self {
            CiPlatform::AzureDevOps => "Azure DevOps",
        }
    }
}

/// Normalised view of the CI environment, regardless of platform.
#[derive(Debug, Clone)]
pub struct CiEnvironment {
    pub platform: CiPlatform,
    /// Working branch name (no refs/heads/ prefix).
    pub branch: String,
    /// PR ID when triggered by a pull request.
    pub pr_id: Option<String>,
    /// PR source branch (same as `branch` in most cases).
    pub pr_source_branch: Option<String>,
    /// PR target branch (e.g. "main").
    pub pr_target_branch: Option<String>,
    /// Platform-specific build identifier.
    pub build_id: Option<String>,
}

impl CiEnvironment {
    /// Returns `Some` when a known CI environment is detected, `None` otherwise.
    pub fn detect() -> Option<Self> {
        Self::detect_azure_devops()
    }

    /// Returns a minimal forced environment for local CI simulation via `[ci] enabled = true`.
    /// Real values come from `CiConfig` overrides in `CiContext::from_environment`.
    pub fn forced() -> Self {
        CiEnvironment {
            platform: CiPlatform::AzureDevOps,
            branch: String::new(),
            pr_id: None,
            pr_source_branch: None,
            pr_target_branch: None,
            build_id: None,
        }
    }

    fn detect_azure_devops() -> Option<Self> {
        if std::env::var("TF_BUILD").ok().as_deref() != Some("True") {
            return None;
        }

        let branch = std::env::var("SYSTEM_PULLREQUEST_SOURCEBRANCH")
            .ok()
            .or_else(|| std::env::var("BUILD_SOURCEBRANCHNAME").ok())?;
        let branch = branch.trim_start_matches("refs/heads/").to_string();

        let pr_id = std::env::var("SYSTEM_PULLREQUEST_PULLREQUESTID").ok();
        let pr_source_branch = std::env::var("SYSTEM_PULLREQUEST_SOURCEBRANCH")
            .ok()
            .map(|b| b.trim_start_matches("refs/heads/").to_string());
        let pr_target_branch = std::env::var("SYSTEM_PULLREQUEST_TARGETBRANCH")
            .ok()
            .map(|b| b.trim_start_matches("refs/heads/").to_string());
        let build_id = std::env::var("BUILD_BUILDID").ok();

        Some(CiEnvironment {
            platform: CiPlatform::AzureDevOps,
            branch,
            pr_id,
            pr_source_branch,
            pr_target_branch,
            build_id,
        })
    }
}

/// Resolved CI context passed to commands.
#[derive(Debug, Clone)]
pub enum CiContext {
    PullRequest {
        pr_id: String,
        source_branch: String,
        target_branch: String,
    },
    Branch {
        branch: String,
    },
}

impl CiContext {
    pub fn from_environment(env: &CiEnvironment, overrides: &CiConfig) -> Self {
        if let Some(pr_id) = overrides.pr_id.clone().or_else(|| env.pr_id.clone()) {
            return CiContext::PullRequest {
                pr_id,
                source_branch: overrides
                    .branch
                    .clone()
                    .unwrap_or_else(|| env.branch.clone()),
                target_branch: overrides
                    .pr_target_branch
                    .clone()
                    .unwrap_or_else(|| env.pr_target_branch.clone().unwrap_or_default()),
            };
        }
        CiContext::Branch {
            branch: overrides
                .branch
                .clone()
                .unwrap_or_else(|| env.branch.clone()),
        }
    }

    /// The branch name to use for `ContextManager::detect`.
    pub fn working_branch(&self) -> &str {
        match self {
            CiContext::PullRequest { source_branch, .. } => source_branch,
            CiContext::Branch { branch } => branch,
        }
    }

    pub fn pr_id(&self) -> Option<&str> {
        match self {
            CiContext::PullRequest { pr_id, .. } => Some(pr_id),
            CiContext::Branch { .. } => None,
        }
    }
}
