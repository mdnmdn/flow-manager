use crate::core::config::Config;
use crate::core::models::WorkItemId;
use crate::providers::factory::ProviderSet;

pub async fn wi_get(id: String) -> anyhow::Result<()> {
    let config = Config::load().ok();
    if let Some(config) = config {
        let provider_set = ProviderSet::from_config(&config)?;
        let tracker = provider_set.issue_tracker;
        let wi = tracker.get_work_item(&WorkItemId(id)).await?;
        println!("{:?}", wi);
    } else {
        println!("Config not found");
    }
    Ok(())
}
