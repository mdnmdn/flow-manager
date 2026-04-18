use crate::core::config::Config;
use crate::core::context::{Context, ContextManager, IdResolution, OutputFormatter};
use crate::core::models::MergeStrategy;
use crate::providers::adonet::AzureDevOpsProvider;
use crate::providers::git::LocalGitProvider;
use crate::providers::{IssueTracker, VCSProvider};
use anyhow::{anyhow, Result};

pub async fn show(id: Option<String>) -> Result<()> {
    let config = Config::load()?;
    let ado = AzureDevOpsProvider::new(&config.ado)?;
    let git = LocalGitProvider;

    let pr_id = if let Some(id_str) = id {
        match ContextManager::resolve_id(&id_str) {
            IdResolution::PullRequest(id) => id,
            IdResolution::WorkItem(wi_id) => {
                let wi = ado.get_work_item(wi_id).await?;
                // Derive branch name as fm does
                let branch_name =
                    ContextManager::derive_branch_name(wi.id, &wi.title, &wi.work_item_type);
                let pr = ado
                    .get_pull_request_by_branch(&config.ado.project, &branch_name)
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
                if let Ok(p) = ado.get_pull_request_details(&config.ado.project, id).await {
                    p.id
                } else {
                    // Try as WI
                    let wi = ado.get_work_item(id).await?;
                    let branch_name =
                        ContextManager::derive_branch_name(wi.id, &wi.title, &wi.work_item_type);
                    let pr = ado
                        .get_pull_request_by_branch(&config.ado.project, &branch_name)
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
        let pr = ado
            .get_pull_request_by_branch(&config.ado.project, &branch)
            .await?;
        match pr {
            Some(p) => p.id,
            None => return Err(anyhow!("No PR found for current branch")),
        }
    };

    let pr = ado
        .get_pull_request_details(&config.ado.project, pr_id)
        .await?;

    let template = "## PR #{{id}} — {{title}}\n\n| Field | Value |\n|---|---|\n| State | {{status}} |\n| Draft | {{is_draft}} |\n| Source | `{{source_branch}}` |\n| Target | `{{target_branch}}` |\n";
    println!(
        "{}",
        OutputFormatter::format(&pr, "markdown", Some(template))?
    );

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
    let ado = AzureDevOpsProvider::new(&config.ado)?;
    let git = LocalGitProvider;
    let branch = git.get_current_branch().await?;

    let pr = ado
        .get_pull_request_by_branch(&config.ado.project, &branch)
        .await?
        .ok_or_else(|| anyhow!("No PR found for current branch"))?;

    let is_draft = if publish { Some(false) } else { None };

    ado.update_pull_request(
        &config.ado.project,
        pr.id,
        title.as_deref(),
        description.as_deref(),
        is_draft,
        status.as_deref(),
    )
    .await?;

    for reviewer in add_reviewers {
        ado.add_reviewer(&config.ado.project, pr.id, &reviewer)
            .await?;
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
    let ado = AzureDevOpsProvider::new(&config.ado)?;
    let git = LocalGitProvider;
    let branch = git.get_current_branch().await?;

    let pr = ado
        .get_pull_request_by_branch(&config.ado.project, &branch)
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

    ado.complete_pull_request(
        &config.ado.project,
        pr.id,
        merge_strategy,
        delete_source_branch,
    )
    .await?;

    // Also close WI if in Activity context
    if let Context::Activity { wi_id, .. } = ContextManager::detect(&branch) {
        ado.update_work_item_state(wi_id, "Closed").await?;
        println!("Work Item #{} closed.", wi_id);
    }

    println!("PR #{} merged successfully.", pr.id);
    Ok(())
}

pub async fn review(id: String) -> Result<()> {
    let config = Config::load()?;
    let ado = AzureDevOpsProvider::new(&config.ado)?;
    let git = LocalGitProvider;

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
        IdResolution::Ambiguous(id) => id,
        _ => return Err(anyhow!("Could not resolve to a PR")),
    };

    let pr = ado
        .get_pull_request_details(&config.ado.project, pr_id)
        .await?;
    let target_branch = pr.source_branch.replace("refs/heads/", "");

    git.fetch().await?;
    git.checkout_branch(&target_branch).await?;

    println!("Now reviewing PR #{} on branch `{}`", pr.id, target_branch);
    Ok(())
}
