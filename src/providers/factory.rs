use crate::core::config::Config;
use crate::providers::adonet::AzureDevOpsProvider;
use crate::providers::{IssueTracker, PipelineProvider, QualityProvider, VCSProvider};
use anyhow::{bail, Result};
use std::sync::Arc;

pub struct ProviderSet {
    pub issue_tracker: Arc<dyn IssueTracker + Send + Sync>,
    pub vcs: Arc<dyn VCSProvider + Send + Sync>,
    pub pipeline: Option<Arc<dyn PipelineProvider + Send + Sync>>,
    pub quality: Option<Arc<dyn QualityProvider + Send + Sync>>,
}

impl ProviderSet {
    pub fn from_config(config: &Config) -> Result<Self> {
        let provider = config
            .provider
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No [provider] section in config. Run `fm init` to create one."))?;

        match provider.kind.as_str() {
            "ado" => {
                let c = provider
                    .ado
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Missing [provider.ado] section in config"))?;
                let ado = Arc::new(AzureDevOpsProvider::new(c)?);
                Ok(ProviderSet {
                    issue_tracker: ado.clone(),
                    vcs: ado.clone(),
                    pipeline: Some(ado),
                    quality: None,
                })
            }
            "github" => todo!("GitHub provider not yet implemented"),
            "gitlab" => todo!("GitLab provider not yet implemented"),
            other => bail!("Unknown provider type '{}'. Expected: ado, github, gitlab", other),
        }
    }
}
