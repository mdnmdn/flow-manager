use config::{Config as ConfigLoader, ConfigError, Environment, File};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ProviderConfig {
    Ado(AdoConfig),
    GitHub(GitHubConfig),
    GitLab(GitLabConfig),
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Config {
    pub provider: Option<ProviderConfig>,
    pub ado: Option<AdoConfig>, // For backward compatibility
    pub sonar: Option<SonarConfig>,
    pub fm: FmConfig,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct AdoConfig {
    pub url: String,
    pub project: String,
    pub pat: String,
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

        // Backward compatibility logic
        let mut config: Config = s.try_deserialize()?;
        if config.provider.is_none() {
            if let Some(ado) = &config.ado {
                config.provider = Some(ProviderConfig::Ado(ado.clone()));
            } else {
                return Err(ConfigError::Message("Missing provider configuration".into()));
            }
        } else if let Some(ProviderConfig::Ado(ado)) = &config.provider {
            if config.ado.is_none() {
                config.ado = Some(ado.clone());
            }
        }

        Ok(config)
    }
}
