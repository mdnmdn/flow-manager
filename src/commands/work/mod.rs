use crate::core::config::Config;
use crate::core::context::{Context, ContextManager, OutputFormatter};
use crate::core::models::WorkItemFilter;
use crate::providers::adonet::AzureDevOpsProvider;
use crate::providers::git::LocalGitProvider;
use crate::providers::sonar::SonarProvider;
use crate::providers::{IssueTracker, QualityProvider, VCSProvider};
use anyhow::{anyhow, Result};
use serde::Serialize;

#[derive(Serialize)]
struct WorkNewResult {
    wi_id: String,
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
    let ado_config = config
        .ado_config()
        .ok_or_else(|| anyhow!("ADO provider not configured"))?;
    let ado = AzureDevOpsProvider::new(ado_config)?;
    let git = LocalGitProvider;
    let target_branch = target.unwrap_or(config.fm.default_target.clone());

    // 1. Create WI
    let wi_type = if type_name == "fix" {
        "Bug"
    } else {
        "User Story"
    };
    let tags_vec: Option<Vec<&str>> = tags.as_ref().map(|t| t.split(';').collect());

    let mut wi = ado
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
                wi = ado
                    .update_work_item(&wi.id, None, Some(&new_desc), None, None)
                    .await?;
            }
        }
    }

    // 3. Derive branch name
    let branch_name = if let Some(slug) = branch_slug {
        format!("{}/{}-{}", type_name, wi.id.as_str(), slug)
    } else {
        ContextManager::derive_branch_name(&wi.id, &wi.title, &type_name)
    };

    // 4. Create remote branch
    ado.create_branch(&ado_config.project, &branch_name, &target_branch)
        .await?;

    // 5. Create draft PR
    let pr = ado
        .create_pull_request(
            &ado_config.project,
            &branch_name,
            &target_branch,
            &title,
            "Draft PR created by fm",
            true,
        )
        .await?;

    // 6. Set WI state to Active
    ado.update_work_item_state(&wi.id, "Active").await?;

    // 7. Local checkout
    git.fetch().await?;
    git.checkout_branch(&branch_name).await?;

    let result = WorkNewResult {
        wi_id: wi.id.to_string(),
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
    let ado_config = config
        .ado_config()
        .ok_or_else(|| anyhow!("ADO provider not configured"))?;
    let ado = AzureDevOpsProvider::new(ado_config)?;
    let git = LocalGitProvider;

    let res = ContextManager::resolve_id(&id);
    let wi_id = match res {
        crate::core::context::IdResolution::WorkItem(id) => id,
        crate::core::context::IdResolution::Ambiguous(id) => id,
        _ => return Err(anyhow!("Could not resolve ID to a Work Item")),
    };

    let wi = ado.get_work_item(&wi_id).await?;
    if wi.state == "Closed" || wi.state == "Done" {
        println!("Work Item #{} is {} and cannot be loaded.", wi.id, wi.state);
        return Ok(());
    }

    let branch_name = match ContextManager::detect(&id) {
        Context::Activity { branch, .. } => branch,
        _ => ContextManager::derive_branch_name(&wi.id, &wi.title, &wi.work_item_type),
    };

    git.fetch().await?;
    if let Err(e) = git.checkout_branch(&branch_name).await {
        return Err(anyhow!("Branch `{}` not found locally or remotely. Run `fm doctor --fix` if you believe this is an error.\nError: {}", branch_name, e));
    }

    // Ensure Active
    ado.update_work_item_state(&wi.id, "Active").await?;

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
    let ado_config = config
        .ado_config()
        .ok_or_else(|| anyhow!("ADO provider not configured"))?;
    let ado = AzureDevOpsProvider::new(ado_config)?;

    let filter_state = if state == "all" { None } else { Some(state) };
    let filter_type = if type_name == "all" {
        None
    } else {
        let actual_type = if type_name == "fix" {
            "Bug"
        } else {
            "User Story"
        };
        Some(actual_type.to_string())
    };

    let filter = WorkItemFilter {
        state: filter_state,
        assigned_to_me: mine,
        work_item_type: filter_type,
        limit: Some(max as u32),
        ..Default::default()
    };

    let items = ado.query_work_items(&filter).await?;
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
