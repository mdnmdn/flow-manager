use crate::core::config::Config;
use crate::providers::adonet::AzureDevOpsProvider;
use crate::providers::git::LocalGitProvider;
use crate::providers::{PipelineProvider, VCSProvider};
use anyhow::{anyhow, Result};
use tokio::time::{sleep, Duration};

pub async fn run(id: Option<String>) -> Result<()> {
    let config = Config::load()?;
    let ado_config = config
        .ado_config()
        .ok_or_else(|| anyhow!("ADO provider not configured"))?;
    let ado = AzureDevOpsProvider::new(ado_config)?;
    let git = LocalGitProvider;
    let branch = git.get_current_branch().await?;

    let pipeline_id = match id {
        Some(s) => s,
        None => {
            let pipelines = ado.list_pipelines().await?;
            println!("Available pipelines:");
            for p in pipelines {
                println!("  ID: {}  Name: {}", p.id, p.name);
            }
            return Err(anyhow::anyhow!("Pipeline ID is required."));
        }
    };

    let run = ado.run_pipeline(&pipeline_id, &branch).await?;
    println!("Pipeline run #{} started. URL: {}", run.id, run.url);

    Ok(())
}

pub async fn status(run_id: Option<String>, watch: bool) -> Result<()> {
    let config = Config::load()?;
    let ado_config = config
        .ado_config()
        .ok_or_else(|| anyhow!("ADO provider not configured"))?;
    let ado = AzureDevOpsProvider::new(ado_config)?;
    let git = LocalGitProvider;
    let branch = git.get_current_branch().await?;

    let id = match run_id {
        Some(s) => s,
        None => {
            let latest = ado.get_latest_run(&branch).await?;
            match latest {
                Some(r) => r.id,
                None => return Err(anyhow::anyhow!("No runs found for branch `{}`", branch)),
            }
        }
    };

    loop {
        let run = ado.get_run_status(&id).await?;
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
