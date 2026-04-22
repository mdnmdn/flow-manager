pub mod feedback;
pub mod thread;

use crate::core::config::Config;
use crate::core::context::{Context, ContextManager, IdResolution};
use crate::core::models::MergeStrategy;
use crate::providers::factory::ProviderSet;
use crate::providers::git::LocalGitProvider;
use crate::providers::{IssueTracker, VCSProvider};
use anyhow::{anyhow, Result};
use regex::Regex;
use std::fmt::Write as FmtWrite;

pub(crate) async fn resolve_pr_id(
    id: Option<String>,
    vcs: &dyn VCSProvider,
    tracker: &dyn IssueTracker,
    git: &LocalGitProvider,
    repo_name: &str,
) -> Result<String> {
    if let Some(id_str) = id {
        match ContextManager::resolve_id(&id_str) {
            IdResolution::PullRequest(id) => Ok(id),
            IdResolution::WorkItem(wi_id) => {
                let wi = tracker.get_work_item(&wi_id).await?;
                let branch_name =
                    ContextManager::derive_branch_name(&wi.id, &wi.title, &wi.work_item_type);
                let pr = vcs
                    .get_pull_request_by_branch(repo_name, &branch_name)
                    .await?;
                match pr {
                    Some(p) => Ok(p.id),
                    None => Err(anyhow!(
                        "No PR found for Work Item #{} (searched branch `{}`)",
                        wi_id,
                        branch_name
                    )),
                }
            }
            IdResolution::Ambiguous(id) => {
                if let Ok(p) = vcs.get_pull_request_details(repo_name, id.as_str()).await {
                    Ok(p.id)
                } else {
                    let wi = tracker.get_work_item(&id).await?;
                    let branch_name =
                        ContextManager::derive_branch_name(&wi.id, &wi.title, &wi.work_item_type);
                    let pr = vcs
                        .get_pull_request_by_branch(repo_name, &branch_name)
                        .await?;
                    match pr {
                        Some(p) => Ok(p.id),
                        None => Err(anyhow!("Could not resolve ID {} to a PR", id)),
                    }
                }
            }
            _ => Err(anyhow!("Invalid ID")),
        }
    } else {
        let branch = git.get_current_branch().await?;
        let pr = vcs.get_pull_request_by_branch(repo_name, &branch).await?;
        match pr {
            Some(p) => Ok(p.id),
            None => Err(anyhow!("No PR found for current branch")),
        }
    }
}

pub fn extract_open_points(description: &str) -> Vec<String> {
    let re = Regex::new(r"- \[[ xX]\] (.+)").unwrap();
    re.captures_iter(description)
        .filter_map(|cap| cap.get(1).map(|m| m.as_str().trim().to_string()))
        .collect()
}

pub async fn show(
    id: Option<String>,
    out: Option<String>,
    include_project_context: bool,
) -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let tracker = provider_set.issue_tracker;
    let vcs = provider_set.vcs;
    let git = LocalGitProvider;

    let repo_name = git.get_repo_name()?;
    let pr_id = resolve_pr_id(id, vcs.as_ref(), tracker.as_ref(), &git, &repo_name).await?;

    let pr = vcs.get_pull_request_details(&repo_name, &pr_id).await?;
    let threads = vcs
        .get_pull_request_threads(&repo_name, &pr_id)
        .await
        .unwrap_or_default();
    let changed_files = vcs
        .get_pull_request_changed_files(&repo_name, &pr_id)
        .await
        .unwrap_or_default();

    let mut doc = String::new();

    let source = pr.source_branch.replace("refs/heads/", "");
    let target = pr.target_branch.replace("refs/heads/", "");
    let author = pr.author.as_deref().unwrap_or("unknown");
    let created_at = pr.created_at.as_deref().unwrap_or("");
    let status = &pr.status;

    writeln!(doc, "# PR #{} — {}", pr.id, pr.title).unwrap();
    writeln!(doc).unwrap();
    writeln!(doc, "**Author:** {}", author).unwrap();
    writeln!(doc, "**Target branch:** {}", target).unwrap();
    writeln!(doc, "**Source branch:** {}", source).unwrap();
    writeln!(doc, "**Created:** {}", created_at).unwrap();
    writeln!(doc, "**Status:** {}", status).unwrap();
    writeln!(doc).unwrap();
    writeln!(doc, "---").unwrap();
    writeln!(doc).unwrap();
    writeln!(doc, "## Description").unwrap();
    writeln!(doc).unwrap();

    let description = pr.description.as_deref().unwrap_or("");
    let open_points = extract_open_points(description);

    if description.is_empty() {
        writeln!(doc, "_No description provided._").unwrap();
    } else {
        writeln!(doc, "{}", description).unwrap();
    }

    if !open_points.is_empty() {
        writeln!(doc).unwrap();
        writeln!(doc, "### Open Points").unwrap();
        writeln!(doc).unwrap();
        for point in &open_points {
            writeln!(doc, "- [ ] {}", point).unwrap();
        }
    }

    writeln!(doc).unwrap();
    writeln!(doc, "---").unwrap();
    writeln!(doc).unwrap();

    let active_threads: Vec<_> = threads.iter().filter(|t| t.status == "active").collect();
    let inactive_threads: Vec<_> = threads.iter().filter(|t| t.status != "active").collect();

    let total = threads.len();
    let active_count = active_threads.len();
    let resolved_count = inactive_threads.len();

    writeln!(
        doc,
        "## Threads\n<!-- total: {}  active: {}  resolved: {} -->",
        total, active_count, resolved_count
    )
    .unwrap();
    writeln!(doc).unwrap();

    let all_threads: Vec<_> = active_threads
        .into_iter()
        .chain(inactive_threads)
        .collect();

    for t in all_threads {
        let date = t.created_at_date.as_deref().unwrap_or("");
        let time = t.created_at_time.as_deref().unwrap_or("");
        let ts = if time.is_empty() {
            t.created_at.clone()
        } else {
            format!("{}T{}", date, time)
        };
        writeln!(doc, "### Thread {} · {}", t.id, t.status).unwrap();
        if let (Some(fp), Some(ln)) = (&t.file_path, t.line) {
            writeln!(doc, "**File:** `{}` line {}", fp, ln).unwrap();
        } else if let Some(fp) = &t.file_path {
            writeln!(doc, "**File:** `{}`", fp).unwrap();
        }
        writeln!(doc, "**Author:** {} · {}", t.author, ts).unwrap();
        writeln!(doc).unwrap();
        writeln!(doc, "{}", t.content).unwrap();
        writeln!(doc).unwrap();
        for reply in &t.replies {
            let rdate = reply.created_at_date.as_deref().unwrap_or("");
            let rtime = reply.created_at_time.as_deref().unwrap_or("");
            let rts = if rtime.is_empty() {
                reply.created_at.clone()
            } else {
                format!("{}T{}", rdate, rtime)
            };
            writeln!(
                doc,
                "> **Reply by {} · {}:** {}",
                reply.author, rts, reply.content
            )
            .unwrap();
            writeln!(doc).unwrap();
        }
        writeln!(doc, "---").unwrap();
        writeln!(doc).unwrap();
    }

    writeln!(doc, "## Changed Files").unwrap();
    writeln!(doc).unwrap();
    writeln!(doc, "| File | Change |").unwrap();
    writeln!(doc, "|---|---|").unwrap();
    for f in &changed_files {
        writeln!(doc, "| {} | {} |", f.path, f.change_type).unwrap();
    }

    if include_project_context {
        let context_files = ["README.md", "AGENTS.md", "CONTRIBUTING.md"];
        let mut has_context = false;
        let mut context_section = String::new();
        for fname in &context_files {
            if let Ok(content) = std::fs::read_to_string(fname) {
                if !has_context {
                    writeln!(context_section, "\n---\n\n## Project Context\n").unwrap();
                    has_context = true;
                }
                writeln!(context_section, "### {}\n", fname).unwrap();
                for line in content.lines().take(60) {
                    writeln!(context_section, "> {}", line).unwrap();
                }
                writeln!(context_section).unwrap();
            }
        }
        if has_context {
            doc.push_str(&context_section);
        }
    }

    if let Some(path) = out {
        std::fs::write(&path, &doc)?;
        eprintln!("Written to {}", path);
    } else {
        print!("{}", doc);
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

    let current_branch = git.get_current_branch().await?;
    if let Context::Activity { wi_id, .. } = ContextManager::detect(&current_branch) {
        let status = git.get_status().await?;
        if !status.is_empty() {
            let stash_msg = format!("stash-{}-review", wi_id);
            git.stash_push(&stash_msg).await?;
        }
        git.push(false).await?;
    }

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

    let pr_id = resolve_pr_id(id, vcs.as_ref(), tracker.as_ref(), &git, &repo_name).await?;

    vcs.add_pull_request_comment(&repo_name, &pr_id, &message)
        .await?;
    println!("Comment added to PR #{}", pr_id);

    Ok(())
}
