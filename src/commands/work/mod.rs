use crate::commands::common::stash_and_push_current_activity;
use crate::core::branch_cache::BranchCache;
use crate::core::config::Config;
use crate::core::context::{Context, ContextManager, IdResolution, OutputFormatter};
use crate::core::models::{WorkItemFilter, WorkItemId};
use crate::providers::factory::ProviderSet;
use crate::providers::git::LocalGitProvider;
use crate::providers::sonar::SonarProvider;
use crate::providers::{QualityProvider, VCSProvider};
use anyhow::{anyhow, Result};
use serde::Serialize;

#[derive(Serialize)]
struct WorkNewResult {
    wi_id: WorkItemId,
    title: String,
    wi_type: String,
    state: String,
    branch: String,
    pr_id: String,
    target: String,
}

#[allow(clippy::too_many_arguments)]
pub async fn run(
    title: String,
    description: Option<String>,
    branch_slug: Option<String>,
    type_name: String,
    target: Option<String>,
    assigned_to: Option<String>,
    tags: Option<String>,
    sonar_project: Option<String>,
) -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let tracker = provider_set.issue_tracker;
    let vcs = provider_set.vcs;
    let git = LocalGitProvider;
    let target_branch = target.unwrap_or(config.fm.default_target.clone());

    // 1. Create WI
    let wi_type = if type_name == "fix" {
        "Bug"
    } else {
        "User Story"
    };
    let tags_vec: Option<Vec<&str>> = tags.as_ref().map(|t| t.split(';').collect());

    let mut wi = tracker
        .create_work_item(
            &title,
            wi_type,
            description.as_deref(),
            assigned_to.as_deref(),
            tags_vec,
        )
        .await?;

    // 2. Sonar issues
    if let Some(project_key) = sonar_project {
        if let Some(sonar_config) = &config.sonar {
            let sonar = SonarProvider::new(sonar_config)?;
            let issues = sonar.get_open_issues(&project_key, None).await?;
            if !issues.is_empty() {
                let mut sonar_desc = String::from("\n\n### Open Sonar Issues\n");
                for issue in issues {
                    sonar_desc.push_str(&format!("- [{}] {}\n", issue.severity, issue.message));
                }
                let new_desc = format!("{}{}", wi.description.unwrap_or_default(), sonar_desc);
                wi = tracker
                    .update_work_item(&wi.id, None, Some(&new_desc), None, None)
                    .await?;
            }
        }
    }

    // 3. Derive branch name
    let branch_name = if let Some(slug) = branch_slug {
        format!("{}/{}-{}", type_name, wi.id, slug)
    } else {
        ContextManager::derive_branch_name(&wi.id, &wi.title, &type_name)
    };

    // 4. Create remote branch
    let repo_name = git.get_repo_name()?;
    vcs.create_branch(&repo_name, &branch_name, &target_branch)
        .await?;

    // 5. Create draft PR
    let is_draft_supported = vcs.capabilities().draft_pull_requests;
    if !is_draft_supported {
        println!("Warning: Draft pull requests are not supported by this provider. Creating a regular PR.");
    }
    let pr = vcs
        .create_pull_request(
            &repo_name,
            &branch_name,
            &target_branch,
            &title,
            "PR created by fm",
            is_draft_supported,
            &[&wi.id],
        )
        .await?;

    // 6. Set WI state to Active
    tracker.update_work_item_state(&wi.id, "Active").await?;

    // 8. Local checkout
    git.fetch().await?;
    git.checkout_branch(&branch_name).await?;
    let cache_wi_type = if wi.work_item_type == "Bug" {
        "fix"
    } else {
        "feature"
    };
    BranchCache::save(&branch_name, &wi.id, cache_wi_type);

    let result = WorkNewResult {
        wi_id: wi.id,
        title: wi.title,
        wi_type: wi.work_item_type,
        state: "Active".to_string(),
        branch: branch_name,
        pr_id: pr.id,
        target: target_branch,
    };

    let template = "## New Activity Started\n\n| | |\n|-|---|\n| Work Item | #{{wi_id}} — {{title}} |\n| Type      | {{wi_type}} |\n| State     | {{state}} |\n| Branch    | `{{branch}}` |\n| PR        | #{{pr_id}} (draft) |\n| Target    | `{{target}}` |\n";
    println!(
        "{}",
        OutputFormatter::format(&result, "markdown", Some(template))?
    );

    Ok(())
}

pub async fn load(id: String, _target: Option<String>) -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let tracker = provider_set.issue_tracker;
    let git = LocalGitProvider;

    let res = ContextManager::resolve_id(&id);
    let wi_id = match res {
        IdResolution::WorkItem(id) => id,
        IdResolution::Ambiguous(id) => id,
        _ => return Err(anyhow!("Could not resolve ID to a Work Item")),
    };

    let wi = tracker.get_work_item(&wi_id).await?;
    if wi.state == "Closed" || wi.state == "Done" {
        println!("Work Item #{} is {} and cannot be loaded.", wi.id, wi.state);
        return Ok(());
    }

    stash_and_push_current_activity(&git).await?;
    BranchCache::clear();

    git.fetch().await?;

    let branch_name = match git.find_branch_for_wi(wi.id.as_str())? {
        Some(b) => b,
        None => {
            // Fall back to artifact links stored in the issue tracker.
            let remote_branches = git.run_git(&["branch", "-r"])?;
            let linked = tracker
                .get_linked_branch_names(&wi.id)
                .await
                .unwrap_or_default();
            linked
                .into_iter()
                .find(|b| {
                    remote_branches
                        .lines()
                        .any(|l| l.trim().trim_start_matches("origin/") == b)
                })
                .unwrap_or_else(|| {
                    ContextManager::derive_branch_name(&wi.id, &wi.title, &wi.work_item_type)
                })
        }
    };

    if let Err(e) = git.checkout_branch(&branch_name).await {
        return Err(anyhow!(
            "Branch `{}` not found locally or remotely.\nError: {}",
            branch_name,
            e
        ));
    }
    // Write branch→WI hint so context detection works even for non-conventional branch names.
    let cache_wi_type = if wi.work_item_type == "Bug" {
        "fix"
    } else {
        "feature"
    };
    BranchCache::save(&branch_name, &wi.id, cache_wi_type);

    // Ensure Active
    tracker.update_work_item_state(&wi.id, "Active").await?;

    // Stash restoration: pop unstaged first, then re-stage the staged stash
    let stash_base = format!("stash-{}-", wi.id);
    let stashes = git.run_git(&["stash", "list"])?;
    let has_unstaged = stashes
        .lines()
        .any(|l| l.contains(&format!("{}unstaged", stash_base)));
    let has_staged = stashes
        .lines()
        .any(|l| l.contains(&format!("{}staged", stash_base)));
    if has_unstaged || has_staged {
        println!("Restoring stash...");
        if has_unstaged {
            git.stash_pop_named(&format!("{}unstaged", stash_base), false)
                .await?;
        }
        if has_staged {
            git.stash_pop_named(&format!("{}staged", stash_base), true)
                .await?;
        }
    }

    println!("Activity #{} loaded: {}", wi.id, wi.title);

    Ok(())
}

pub async fn list(mine: bool, state: String, type_name: String, max: i32) -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let tracker = provider_set.issue_tracker;

    let mut filter = WorkItemFilter {
        limit: Some(max as u32),
        ..Default::default()
    };

    if mine {
        filter.assigned_to = Some("@Me".to_string());
    }

    if state != "all" {
        filter.state = Some(state);
    }

    if type_name != "all" {
        let actual_type = if type_name == "fix" {
            "Bug"
        } else {
            "User Story"
        };
        filter.work_item_type = Some(actual_type.to_string());
    }

    let items = tracker.query_work_items(&filter).await?;
    let limited_items: Vec<_> = items.into_iter().take(max as usize).collect();

    println!("| ID | Type | State | Title | Assigned To |");
    println!("|----|------|-------|-------|-------------|");
    for item in limited_items {
        println!(
            "| #{} | {} | {} | {} | {} |",
            item.id,
            item.work_item_type,
            item.state,
            item.title,
            item.assigned_to.unwrap_or_else(|| "unassigned".to_string())
        );
    }

    Ok(())
}

pub async fn show(id: String, include_comments: bool, compact: bool) -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let tracker = provider_set.issue_tracker;
    let vcs = provider_set.vcs;
    let git = LocalGitProvider;

    let (wi_id, branch) = if id.is_empty() {
        let branch = git.get_current_branch().await?;
        match ContextManager::detect(&branch) {
            Context::Activity { wi_id, .. } => (wi_id, Some(branch)),
            _ => return Err(anyhow!("Not in an Activity context - provide a task id or run from an activity branch")),
        }
    } else {
        let res = ContextManager::resolve_id(&id);
        let wi_id = match res {
            IdResolution::WorkItem(id) => id,
            IdResolution::Ambiguous(id) => id,
            _ => return Err(anyhow!("Could not resolve ID to a Work Item")),
        };
        (wi_id, None)
    };

    let wi = tracker.get_work_item(&wi_id).await?;
    let comments = tracker.get_work_item_comments(&wi_id).await?;

    let pr = if let Some(b) = &branch {
        let repo_name = git.get_repo_name()?;
        vcs.get_pull_request_by_branch(&repo_name, b).await.ok().flatten()
    } else {
        None
    };

    let comments_count = comments.len() as i32;

    if compact {
        println!("#{} [{}] - {}", wi.id, wi.state, wi.title);
        if let Some(p) = &pr {
            println!("PR: {} - {}", p.id, p.source_branch);
        }
        println!("Comments: {}", comments_count);
        return Ok(());
    }

    println!("## {} [{}] - {}", wi.id, wi.state, wi.title);

    if let Some(p) = &pr {
        println!("\nPR: {} - {}", p.id, p.source_branch);
    }

    if let Some(desc) = &wi.description {
        if !desc.is_empty() {
            println!("\n{}", desc);
        }
    }

    if include_comments {
        for comment in &comments {
            let date = comment.created_at_date.as_deref().unwrap_or("");
            let time = comment.created_at_time.as_deref().unwrap_or("");
            println!("\n### {} {} - {}", date, time, comment.author);
            println!("\n{}", comment.text);
        }
    }

    Ok(())
}
