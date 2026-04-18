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
            let status = git.run_git(&["status", "--porcelain", sub])?;
            if !status.is_empty() {
                println!(
                    "Submodule `{}` has pending pointer update. Committing it...",
                    sub
                );
                git.run_git(&[
                    "commit",
                    "-m",
                    &format!("chore: update {} submodule pointer", sub),
                    sub,
                ])?;
            }
        }
    }

    git.push(force).await?;
    println!("Pushed to remote.");
    Ok(())
}
