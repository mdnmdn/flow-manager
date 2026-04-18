use crate::core::config::Config;
use crate::providers::adonet::AzureDevOpsProvider;
use crate::providers::IssueTracker;

pub async fn wi_get(id: i32) -> anyhow::Result<()> {
    let config = Config::load().ok();
    if let Some(config) = config {
        let provider = AzureDevOpsProvider::new(&config.ado)?;
        let wi = provider.get_work_item(id).await?;
        println!("{:?}", wi);
    } else {
        println!("Config not found");
    }
    Ok(())
}
