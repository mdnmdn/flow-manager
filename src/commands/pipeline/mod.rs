use crate::core::config::Config;
use crate::providers::factory::ProviderSet;
use crate::providers::git::LocalGitProvider;
use crate::providers::VCSProvider;
use anyhow::Result;
use tokio::time::{sleep, Duration};

pub async fn run(id: Option<String>) -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let pipeline = provider_set
        .pipeline
        .ok_or_else(|| anyhow::anyhow!("Pipeline provider not supported for this provider"))?;
    let git = LocalGitProvider;
    let branch = git.get_current_branch().await?;

    let pipeline_id = match id {
        Some(i) => i,
        None => {
            let pipelines = pipeline.list_pipelines().await?;
            println!("Available pipelines:");
            for p in pipelines {
                println!("  ID: {}  Name: {}", p.id, p.name);
            }
            return Err(anyhow::anyhow!("Pipeline ID is required."));
        }
    };

    let run = pipeline.run_pipeline(&pipeline_id, &branch).await?;
    println!("Pipeline run #{} started. URL: {}", run.id, run.url);

    Ok(())
}

pub async fn status(run_id: Option<String>, watch: bool) -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let pipeline = provider_set
        .pipeline
        .ok_or_else(|| anyhow::anyhow!("Pipeline provider not supported for this provider"))?;
    let git = LocalGitProvider;
    let branch = git.get_current_branch().await?;

    let id = match run_id {
        Some(i) => i,
        None => {
            let latest = pipeline.get_latest_run(&branch).await?;
            match latest {
                Some(r) => r.id,
                None => return Err(anyhow::anyhow!("No runs found for branch `{}`", branch)),
            }
        }
    };

    loop {
        let run = pipeline.get_run_status(&id).await?;
        println!(
            "Run #{} Status: {} Result: {:?}",
            run.id, run.status, run.result
        );

        if !watch || run.status == "completed" {
            break;
        }
        sleep(Duration::from_secs(30)).await;
    }

    Ok(())
}
