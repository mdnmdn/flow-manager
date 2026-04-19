use crate::core::config::Config;
use crate::core::models::WorkItemId;
use crate::providers::adonet::AzureDevOpsProvider;
use crate::providers::IssueTracker;

pub async fn wi_get(id: String) -> anyhow::Result<()> {
    let config = Config::load().ok();
    if let Some(config) = config {
        if let Some(ado_config) = config.ado_config() {
            let provider = AzureDevOpsProvider::new(ado_config)?;
            let wi_id = WorkItemId(id);
            let wi = provider.get_work_item(&wi_id).await?;
            println!("{:?}", wi);
        } else {
            println!("ADO provider not configured");
        }
    } else {
        println!("Config not found");
    }
    Ok(())
}
