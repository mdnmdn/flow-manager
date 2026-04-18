use crate::core::config::Config;
use crate::providers::git::LocalGitProvider;
use crate::providers::VCSProvider;
use anyhow::Result;

pub async fn run(force: bool, no_docs: bool) -> Result<()> {
    let config = Config::load()?;
    let git = LocalGitProvider;

    if !no_docs {
        for sub in &config.fm.submodules {
            // Check if submodule pointer was updated but not committed
            // We use status here, maybe a better method in VCSProvider?
            let status = git.get_status().await?;
            if status.contains(sub) {
                println!(
                    "Submodule `{}` has pending pointer update. Committing it...",
                    sub
                );
                git.commit(
                    &format!("chore: update {} submodule pointer", sub),
                    false,
                    false,
                )
                .await?;
            }
        }
    }

    git.push(force).await?;
    println!("Pushed to remote.");
    Ok(())
}
