use config::{Config as ConfigLoader, ConfigError, Environment, File};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ProviderConfig {
    #[serde(rename = "type")]
    pub kind: String,
    pub ado: Option<AdoConfig>,
    pub github: Option<GitHubConfig>,
    pub gitlab: Option<GitLabConfig>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    pub provider: Option<ProviderConfig>,
    pub sonar: Option<SonarConfig>,
    pub fm: FmConfig,
    #[serde(default)]
    pub ci: CiConfig,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct CiConfig {
    /// Force CI mode even when not in a real pipeline.
    #[serde(default)]
    pub enabled: bool,
    /// Override the detected branch name.
    pub branch: Option<String>,
    /// Override the detected PR ID.
    pub pr_id: Option<String>,
    /// Override the detected PR target branch.
    pub pr_target_branch: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AdoConfig {
    pub url: String,
    pub project: String,
    pub pat: String,
    #[serde(default = "default_todo_wi_type")]
    pub todo_wi_type: String,
    #[serde(default = "default_bug_wi_type")]
    pub bug_wi_type: String,
    #[serde(default = "default_in_progress")]
    pub todo_in_progress_status: String,
    #[serde(default = "default_complete")]
    pub todo_complete_status: String,
    #[serde(default = "default_in_progress")]
    pub default_in_progress_status: String,
    pub default_area: Option<String>,
    #[serde(default)]
    pub default_current_iteration: bool,
    #[serde(default = "default_assign_to_me")]
    pub default_assign_to_me: bool,
}

fn default_assign_to_me() -> bool {
    true
}

fn default_todo_wi_type() -> String {
    "Task".to_string()
}

fn default_bug_wi_type() -> String {
    "Bug".to_string()
}

fn default_in_progress() -> String {
    "In Progress".to_string()
}

fn default_complete() -> String {
    "Done".to_string()
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GitHubConfig {
    pub token: String,
    pub owner: String,
    pub repo: String,
    pub base_url: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct GitLabConfig {
    pub token: String,
    pub namespace: String,
    pub project_id: Option<u64>,
    pub base_url: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SonarConfig {
    pub url: String,
    pub token: String,
    #[serde(default)]
    pub projects: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct FmConfig {
    #[serde(default = "default_merge_strategy")]
    pub merge_strategy: String,
    #[serde(default = "default_target_branch")]
    pub default_target: String,
    #[serde(default = "default_wi_type")]
    pub default_wi_type: String,
    #[serde(default)]
    pub submodules: Vec<String>,
}

fn default_merge_strategy() -> String {
    "squash".to_string()
}

fn default_target_branch() -> String {
    "main".to_string()
}

fn default_wi_type() -> String {
    "User Story".to_string()
}

impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        let s = ConfigLoader::builder()
            // Start with default values if any
            .set_default("fm.merge_strategy", "squash")?
            .set_default("fm.default_target", "main")?
            .set_default("fm.default_wi_type", "User Story")?
            .set_default("fm.submodules", Vec::<String>::new())?
            // Add configuration from files
            .add_source(File::with_name("fm").required(false))
            .add_source(File::with_name(".env").required(false))
            // Add configuration from environment variables (with a prefix like FM_)
            // e.g. FM_ADO_URL will set ado.url
            .add_source(Environment::with_prefix("FM").separator("__"))
            .build()?;

        let mut cfg: Config = s.try_deserialize()?;

        // ADO convenience fallback: populate url/project from pipeline vars when absent.
        if let Some(ref mut provider) = cfg.provider {
            if provider.kind == "ado" {
                if let Some(ref mut ado) = provider.ado {
                    if ado.url.is_empty() {
                        if let Ok(uri) = std::env::var("SYSTEM_TEAMFOUNDATIONCOLLECTIONURI") {
                            ado.url = uri.trim_end_matches('/').to_string();
                        }
                    }
                    if ado.project.is_empty() {
                        if let Ok(proj) = std::env::var("SYSTEM_TEAMPROJECT") {
                            ado.project = proj;
                        }
                    }
                }
            }
        }

        Ok(cfg)
    }
}
