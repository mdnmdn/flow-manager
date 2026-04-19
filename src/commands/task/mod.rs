use crate::commands::common::stash_and_push_current_activity;
use crate::core::branch_cache::BranchCache;
use crate::core::config::Config;
use crate::core::context::{Context, ContextManager};
use crate::providers::factory::ProviderSet;
use crate::providers::git::LocalGitProvider;
use crate::providers::VCSProvider;
use anyhow::{anyhow, Result};

pub async fn hold(force: bool, stay: bool) -> Result<()> {
    let git = LocalGitProvider;
    let branch = git.get_current_branch().await?;

    if let Context::Baseline { .. } = ContextManager::detect(&branch) {
        println!("Already on baseline, nothing to hold.");
        return Ok(());
    }

    if force {
        let status = git.get_status().await?;
        if !status.is_empty() {
            git.discard_local_changes().await?;
        }
    }

    stash_and_push_current_activity(&git).await?;
    BranchCache::clear();

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
    let provider_set = ProviderSet::from_config(&config)?;
    let tracker = provider_set.issue_tracker;
    let git = LocalGitProvider;
    let branch = git.get_current_branch().await?;
    let context = ContextManager::detect(&branch);

    let wi_id = match context {
        Context::Activity { wi_id, .. } => wi_id,
        _ => return Err(anyhow!("Not in an Activity context")),
    };

    let tags_vec: Option<Vec<&str>> = tags.as_ref().map(|t| t.split(';').collect());

    let updated = tracker
        .update_work_item(
            &wi_id,
            title.as_deref(),
            description.as_deref(),
            assigned_to.as_deref(),
            tags_vec,
        )
        .await?;
    if let Some(s) = state {
        tracker.update_work_item_state(&wi_id, &s).await?;
    }

    println!("Work Item #{} updated.", updated.id);
    Ok(())
}

pub async fn complete() -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let tracker = provider_set.issue_tracker;
    let vcs = provider_set.vcs;
    let git = LocalGitProvider;
    let branch = git.get_current_branch().await?;
    let context = ContextManager::detect(&branch);

    let wi_id = match context {
        Context::Activity { wi_id, .. } => wi_id,
        _ => return Err(anyhow!("Already on baseline — nothing to complete.")),
    };

    let wi = tracker.get_work_item(&wi_id).await?;
    let repo_name = git.get_repo_name()?;
    let pr = vcs.get_pull_request_by_branch(&repo_name, &branch).await?;

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
    BranchCache::clear();

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

    git.fetch().await?;
    let target = format!("origin/{}", config.fm.default_target);

    if check {
        let ahead = git
            .get_log(Some(&format!("{}..HEAD", target)), None)
            .await?;
        let behind = git
            .get_log(Some(&format!("HEAD..{}", target)), None)
            .await?;
        println!("## Sync Check\n\nAhead:\n{}\n\nBehind:\n{}", ahead, behind);
        return Ok(());
    }

    if rebase {
        git.rebase(&target).await?;
    } else {
        git.merge(&target).await?;
    }

    git.push(false).await?;
    println!("Synced with `{}` and pushed.", target);

    Ok(())
}

pub async fn comment(message: String) -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let tracker = provider_set.issue_tracker;
    let git = LocalGitProvider;
    let branch = git.get_current_branch().await?;
    let context = ContextManager::detect(&branch);

    let wi_id = match context {
        Context::Activity { wi_id, .. } => wi_id,
        _ => return Err(anyhow!("Not in an Activity context")),
    };

    let new_comment = tracker.add_work_item_comment(&wi_id, &message).await?;
    println!("Comment added to WI #{}: {}", wi_id, new_comment.text);

    Ok(())
}
