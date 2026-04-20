use crate::core::config::Config;
use crate::core::context::{Context, ContextManager, IdResolution};
use crate::core::models::MergeStrategy;
use crate::providers::factory::ProviderSet;
use crate::providers::git::LocalGitProvider;
use crate::providers::VCSProvider;
use anyhow::{anyhow, Result};

pub async fn show(id: Option<String>, include_comments: bool, compact: bool) -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let tracker = provider_set.issue_tracker;
    let vcs = provider_set.vcs;
    let git = LocalGitProvider;

    let repo_name = git.get_repo_name()?;

    let pr_id = if let Some(id_str) = id {
        match ContextManager::resolve_id(&id_str) {
            IdResolution::PullRequest(id) => id,
            IdResolution::WorkItem(wi_id) => {
                let wi = tracker.get_work_item(&wi_id).await?;
                // Derive branch name as fm does
                let branch_name =
                    ContextManager::derive_branch_name(&wi.id, &wi.title, &wi.work_item_type);
                let pr = vcs
                    .get_pull_request_by_branch(&repo_name, &branch_name)
                    .await?;
                match pr {
                    Some(p) => p.id,
                    None => {
                        return Err(anyhow!(
                            "No PR found for Work Item #{} (searched branch `{}`)",
                            wi_id,
                            branch_name
                        ))
                    }
                }
            }
            IdResolution::Ambiguous(id) => {
                // Try as PR first
                if let Ok(p) = vcs.get_pull_request_details(&repo_name, id.as_str()).await {
                    p.id
                } else {
                    // Try as WI
                    let wi = tracker.get_work_item(&id).await?;
                    let branch_name =
                        ContextManager::derive_branch_name(&wi.id, &wi.title, &wi.work_item_type);
                    let pr = vcs
                        .get_pull_request_by_branch(&repo_name, &branch_name)
                        .await?;
                    match pr {
                        Some(p) => p.id,
                        None => return Err(anyhow!("Could not resolve ID {} to a PR", id)),
                    }
                }
            }
            _ => return Err(anyhow!("Invalid ID")),
        }
    } else {
        let branch = git.get_current_branch().await?;
        let pr = vcs.get_pull_request_by_branch(&repo_name, &branch).await?;
        match pr {
            Some(p) => p.id,
            None => return Err(anyhow!("No PR found for current branch")),
        }
    };

    let pr = vcs.get_pull_request_details(&repo_name, &pr_id).await?;
    let pr_comments = vcs
        .get_pull_request_comments(&repo_name, &pr_id)
        .await
        .unwrap_or_default();

    let comments_count = pr_comments.len() as i32;

    if compact {
        let draft = if pr.is_draft { "draft" } else { "active" };
        println!("#{} [{}] - {}", pr.id, draft, pr.title);
        println!("{} -> {}", pr.source_branch, pr.target_branch);
        println!("Comments: {}", comments_count);
        return Ok(());
    }

    println!("## {} [{}] - {}", pr.id, pr.status, pr.title);
    println!("\n{} -> {}", pr.source_branch, pr.target_branch);

    if include_comments {
        for comment in pr_comments {
            let date = comment.created_at_date.as_deref().unwrap_or("");
            let time = comment.created_at_time.as_deref().unwrap_or("");
            println!("\n### {} {} - {}", date, time, comment.author);
            println!("\n{}", comment.content);
            for reply in &comment.replies {
                let rdate = reply.created_at_date.as_deref().unwrap_or("");
                let rtime = reply.created_at_time.as_deref().unwrap_or("");
                println!("\n> **{} {} - {}**", rdate, rtime, reply.author);
                for line in reply.content.lines() {
                    println!("> {}", line);
                }
            }
        }
    }

    Ok(())
}

pub async fn update(
    title: Option<String>,
    description: Option<String>,
    publish: bool,
    status: Option<String>,
    add_reviewers: Vec<String>,
) -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let vcs = provider_set.vcs;
    let git = LocalGitProvider;
    let branch = git.get_current_branch().await?;

    let repo_name = git.get_repo_name()?;

    let pr = vcs
        .get_pull_request_by_branch(&repo_name, &branch)
        .await?
        .ok_or_else(|| anyhow!("No PR found for current branch"))?;

    let is_draft = if publish { Some(false) } else { None };

    vcs.update_pull_request(
        &repo_name,
        &pr.id,
        title.as_deref(),
        description.as_deref(),
        is_draft,
        status.as_deref(),
    )
    .await?;

    for reviewer in add_reviewers {
        vcs.add_reviewer(&repo_name, &pr.id, &reviewer).await?;
    }

    println!("PR #{} updated.", pr.id);
    Ok(())
}

pub async fn merge(
    strategy: Option<String>,
    delete_source_branch: bool,
    _bypass_policy: bool,
) -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let tracker = provider_set.issue_tracker;
    let vcs = provider_set.vcs;
    let git = LocalGitProvider;
    let branch = git.get_current_branch().await?;

    let repo_name = git.get_repo_name()?;

    let pr = vcs
        .get_pull_request_by_branch(&repo_name, &branch)
        .await?
        .ok_or_else(|| anyhow!("No PR found for current branch"))?;

    if pr.is_draft {
        return Err(anyhow!("PR #{} is still a draft. Publish it first.", pr.id));
    }

    let merge_strategy = match strategy.as_deref().unwrap_or(&config.fm.merge_strategy) {
        "squash" => MergeStrategy::Squash,
        "rebase" => MergeStrategy::Rebase,
        "rebaseMerge" => MergeStrategy::RebaseMerge,
        "noFastForward" => MergeStrategy::NoFastForward,
        _ => MergeStrategy::Squash,
    };

    if !vcs
        .capabilities()
        .merge_strategies
        .contains(&merge_strategy)
    {
        return Err(anyhow!(
            "Merge strategy `{}` is not supported by this provider.",
            merge_strategy
        ));
    }

    vcs.complete_pull_request(&repo_name, &pr.id, merge_strategy, delete_source_branch)
        .await?;

    // Also close WI if in Activity context
    if let Context::Activity { wi_id, .. } = ContextManager::detect(&branch) {
        tracker.update_work_item_state(&wi_id, "Closed").await?;
        println!("Work Item #{} closed.", wi_id);
    }

    println!("PR #{} merged successfully.", pr.id);
    Ok(())
}

pub async fn review(id: String) -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let vcs = provider_set.vcs;
    let git = LocalGitProvider;

    let repo_name = git.get_repo_name()?;

    // 1. Pause current
    let current_branch = git.get_current_branch().await?;
    if let Context::Activity { wi_id, .. } = ContextManager::detect(&current_branch) {
        let status = git.get_status().await?;
        if !status.is_empty() {
            let stash_msg = format!("stash-{}-review", wi_id);
            git.stash_push(&stash_msg).await?;
        }
        git.push(false).await?;
    }

    // 2. Resolve target PR
    let pr_id = match ContextManager::resolve_id(&id) {
        IdResolution::PullRequest(id) => id,
        IdResolution::Ambiguous(id) => id.to_string(),
        _ => return Err(anyhow!("Could not resolve to a PR")),
    };

    let pr = vcs.get_pull_request_details(&repo_name, &pr_id).await?;
    let target_branch = pr.source_branch.replace("refs/heads/", "");

    git.fetch().await?;
    git.checkout_branch(&target_branch).await?;

    println!("Now reviewing PR #{} on branch `{}`", pr.id, target_branch);
    Ok(())
}

pub async fn comment(id: Option<String>, message: String) -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let vcs = provider_set.vcs;
    let git = LocalGitProvider;
    let tracker = provider_set.issue_tracker;

    let repo_name = git.get_repo_name()?;

    let pr_id = if let Some(id_str) = id {
        match ContextManager::resolve_id(&id_str) {
            IdResolution::PullRequest(id) => id,
            IdResolution::WorkItem(wi_id) => {
                let wi = tracker.get_work_item(&wi_id).await?;
                let branch_name =
                    ContextManager::derive_branch_name(&wi.id, &wi.title, &wi.work_item_type);
                let pr = vcs
                    .get_pull_request_by_branch(&repo_name, &branch_name)
                    .await?;
                pr.ok_or_else(|| anyhow!("No PR found for WI"))?.id
            }
            IdResolution::Ambiguous(id) => {
                if let Ok(p) = vcs.get_pull_request_details(&repo_name, id.as_str()).await {
                    p.id
                } else {
                    let wi = tracker.get_work_item(&id).await?;
                    let branch_name =
                        ContextManager::derive_branch_name(&wi.id, &wi.title, &wi.work_item_type);
                    let pr = vcs
                        .get_pull_request_by_branch(&repo_name, &branch_name)
                        .await?;
                    pr.ok_or_else(|| anyhow!("No PR found"))?.id
                }
            }
            _ => return Err(anyhow!("Invalid ID")),
        }
    } else {
        let branch = git.get_current_branch().await?;
        let pr = vcs.get_pull_request_by_branch(&repo_name, &branch).await?;
        pr.ok_or_else(|| anyhow!("No PR found for current branch"))?
            .id
    };

    vcs.add_pull_request_comment(&repo_name, &pr_id, &message)
        .await?;
    println!("Comment added to PR #{}", pr_id);

    Ok(())
}
