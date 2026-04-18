use crate::core::config::Config;
use crate::core::context::{Context, ContextManager};
use crate::providers::adonet::AzureDevOpsProvider;
use crate::providers::git::LocalGitProvider;
use crate::providers::{IssueTracker, VCSProvider};
use anyhow::Result;

pub async fn run(
    message: Option<String>,
    all: bool,
    amend: bool,
    docs_message: Option<String>,
    no_docs: bool,
) -> Result<()> {
    let config = Config::load()?;
    let git = LocalGitProvider;

    // 1. Handle submodules
    // 1. Handle submodules
    if !no_docs {
        for sub in &config.fm.submodules {
            if git.check_submodule_status(sub).await? {
                println!("Handling submodule `{}`...", sub);
                // Submodule handling remains somewhat git-specific for now because it involves "-C" or path context.
                // However, we could potentially add methods to VCSProvider for submodule operations.
                let msg = docs_message.clone().unwrap_or_else(|| {
                    format!("docs: {}", message.as_deref().unwrap_or("update docs"))
                });
                // Commit in submodule (this is still using run_git but in the context of the submodule)
                git.run_git(&["-C", sub, "add", "-A"])?;
                git.run_git(&["-C", sub, "commit", "-m", &msg])?;
                git.run_git(&["-C", sub, "push"])?;
                // Update pointer in main repo
                git.update_submodule_pointer(sub).await?;
            }
        }
    }

    // 2. Commit main repo
    let branch = git.get_current_branch().await?;
    let commit_msg = match message {
        Some(m) => m,
        None => {
            if let Context::Activity { wi_id, .. } = ContextManager::detect(&branch) {
                let ado = AzureDevOpsProvider::new(&config.ado)?;
                let wi = ado.get_work_item(wi_id).await?;
                format!("[#{}] {}: work in progress", wi.id, wi.title)
            } else {
                return Err(anyhow::anyhow!(
                    "Commit message is required in baseline context"
                ));
            }
        }
    };

    let mut args = vec!["commit", "-m", &commit_msg];
    if all {
        args.push("-a");
    }
    git.commit(&commit_msg, all, amend).await?;
    println!("Committed to `{}`", branch);

    Ok(())
}
