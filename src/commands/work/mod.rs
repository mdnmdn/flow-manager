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

    let branch_name = match ContextManager::detect(&id) {
        Context::Activity { branch, .. } => branch,
        _ => ContextManager::derive_branch_name(&wi.id, &wi.title, &wi.work_item_type),
    };

    // Check if branch exists, if not error out (as per user instructions to use doctor --fix)
    // Actually, user said "abort on most command, leave only some 'main' command to fix"
    // So here I should check if I can checkout.

    git.fetch().await?;
    if let Err(e) = git.checkout_branch(&branch_name).await {
        return Err(anyhow!("Branch `{}` not found locally or remotely. Run `fm doctor --fix` if you believe this is an error.\nError: {}", branch_name, e));
    }

    // Ensure Active
    tracker.update_work_item_state(&wi.id, "Active").await?;

    // Stash restoration
    let stash_name = format!("stash-{}-", wi.id);
    let stashes = git.run_git(&["stash", "list"])?;
    for line in stashes.lines() {
        if line.contains(&stash_name) {
            println!("Restoring stash...");
            git.stash_pop().await?;
            break;
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
