use crate::core::config::Config;
use crate::core::context::{Context, ContextManager, OutputFormatter};
use crate::providers::adonet::AzureDevOpsProvider;
use crate::providers::git::LocalGitProvider;
use crate::providers::{IssueTracker, VCSProvider};
use anyhow::{anyhow, Result};

pub async fn hold(stash: bool, force: bool, stay: bool) -> Result<()> {
    let git = LocalGitProvider;
    let branch = git.get_current_branch().await?;
    let context = ContextManager::detect(&branch);

    let (wi_id, slug) = match context {
        Context::Baseline { .. } => {
            println!("Already on baseline, nothing to hold.");
            return Ok(());
        }
        Context::Activity { wi_id, branch, .. } => {
            let slug = branch.split('/').last().unwrap_or("activity");
            (wi_id, slug.to_string())
        }
    };

    let status = git.get_status().await?;
    if !status.is_empty() {
        if stash {
            let stash_msg = format!("stash-{}-{}", wi_id, slug);
            git.stash_push(&stash_msg).await?;
        } else if force {
            git.run_git(&["checkout", "--", "."])?;
        } else {
            println!(
                "Uncommitted changes present. Use `--stash` to save them or `--force` to discard."
            );
            println!("{}", status);
            return Err(anyhow!("Working tree dirty"));
        }
    }

    git.push(false).await?;

    if !stay {
        let config = Config::load()?;
        git.checkout_branch(&config.fm.default_target).await?;
        println!("Moved to baseline `{}`", config.fm.default_target);
    }

    Ok(())
}

pub async fn update(
    title: Option<String>,
    state: Option<String>,
    description: Option<String>,
    assigned_to: Option<String>,
    tags: Option<String>,
) -> Result<()> {
    let config = Config::load()?;
    let ado = AzureDevOpsProvider::new(&config.ado)?;
    let git = LocalGitProvider;
    let branch = git.get_current_branch().await?;
    let context = ContextManager::detect(&branch);

    let wi_id = match context {
        Context::Activity { wi_id, .. } => wi_id,
        _ => return Err(anyhow!("Not in an Activity context")),
    };

    let tags_vec: Option<Vec<&str>> = tags.as_ref().map(|t| t.split(';').collect());

    let updated = ado
        .update_work_item(
            wi_id,
            title.as_deref(),
            description.as_deref(),
            assigned_to.as_deref(),
            tags_vec,
        )
        .await?;
    if let Some(s) = state {
        ado.update_work_item_state(wi_id, &s).await?;
    }

    println!("Work Item #{} updated.", updated.id);
    Ok(())
}

pub async fn complete() -> Result<()> {
    let config = Config::load()?;
    let ado = AzureDevOpsProvider::new(&config.ado)?;
    let git = LocalGitProvider;
    let branch = git.get_current_branch().await?;
    let context = ContextManager::detect(&branch);

    let wi_id = match context {
        Context::Activity { wi_id, .. } => wi_id,
        _ => return Err(anyhow!("Already on baseline — nothing to complete.")),
    };

    let wi = ado.get_work_item(wi_id).await?;
    let pr = ado
        .get_pull_request_by_branch(&config.ado.project, &branch)
        .await?;

    if let Some(p) = pr {
        if p.status != "completed" && p.status != "abandoned" {
            return Err(anyhow!(
                "PR #{} is still {}. Merge or abandon it first.",
                p.id,
                p.status
            ));
        }
    }

    if wi.state != "Closed" && wi.state != "Done" {
        // Silently close if PR is merged? The spec says error if not closed.
        // Let's be strict.
        return Err(anyhow!(
            "Work Item #{} is still {}. Close it first.",
            wi.id,
            wi.state
        ));
    }

    git.checkout_branch(&config.fm.default_target).await?;
    git.pull().await?;

    println!("Activity complete. Now on `{}`", config.fm.default_target);
    Ok(())
}

pub async fn sync(rebase: bool, check: bool) -> Result<()> {
    let config = Config::load()?;
    let git = LocalGitProvider;
    let branch = git.get_current_branch().await?;

    if let Context::Baseline { .. } = ContextManager::detect(&branch) {
        return Err(anyhow!("Already on baseline — nothing to sync."));
    }

    git.run_git(&["fetch", "origin"])?;
    let target = format!("origin/{}", config.fm.default_target);

    if check {
        let ahead = git.run_git(&["log", "--oneline", &format!("{}..HEAD", target)])?;
        let behind = git.run_git(&["log", "--oneline", &format!("HEAD..{}", target)])?;
        println!("## Sync Check\n\nAhead:\n{}\n\nBehind:\n{}", ahead, behind);
        return Ok(());
    }

    if rebase {
        git.run_git(&["rebase", &target])?;
    } else {
        git.run_git(&["merge", &target])?;
    }

    git.push(false).await?;
    println!("Synced with `{}` and pushed.", target);

    Ok(())
}
