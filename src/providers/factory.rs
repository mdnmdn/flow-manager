use crate::core::config::{Config, ProviderConfig};
use crate::providers::adonet::AzureDevOpsProvider;
use crate::providers::{IssueTracker, PipelineProvider, QualityProvider, VCSProvider};
use anyhow::Result;
use std::sync::Arc;

pub struct ProviderSet {
    pub issue_tracker: Arc<dyn IssueTracker + Send + Sync>,
    pub vcs: Arc<dyn VCSProvider + Send + Sync>,
    pub pipeline: Option<Arc<dyn PipelineProvider + Send + Sync>>,
    pub quality: Option<Arc<dyn QualityProvider + Send + Sync>>,
}

impl ProviderSet {
    pub fn from_config(config: &Config) -> Result<Self> {
        match config.provider.as_ref().unwrap() {
            ProviderConfig::Ado(c) => {
                let ado = Arc::new(AzureDevOpsProvider::new(c)?);
                Ok(ProviderSet {
                    issue_tracker: ado.clone(),
                    vcs: ado.clone(),
                    pipeline: Some(ado),
                    quality: None, // Will be wired if Sonar is present
                })
            }
            ProviderConfig::GitHub(_c) => {
                todo!("GitHub provider not yet implemented")
            }
            ProviderConfig::GitLab(_c) => {
                todo!("GitLab provider not yet implemented")
            }
        }
    }
}
