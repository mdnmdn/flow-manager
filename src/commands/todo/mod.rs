use crate::core::config::Config;
use crate::core::context::{Context, ContextManager};
use crate::core::models::WorkItemId;
use crate::providers::factory::ProviderSet;
use crate::providers::git::LocalGitProvider;
use crate::providers::IssueTracker;
use crate::providers::VCSProvider;
use anyhow::{anyhow, Result};

pub async fn show(all: bool, detail: bool) -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let tracker = provider_set.issue_tracker;

    if !tracker.capabilities().work_item_hierarchy {
        return Err(anyhow!(
            "Work item hierarchy is not supported by this provider. Todo commands are unavailable."
        ));
    }

    let git = LocalGitProvider;
    let branch = git.get_current_branch().await?;

    let wi_id = match ContextManager::detect(&branch) {
        Context::Activity { wi_id, .. } => wi_id,
        _ => return Err(anyhow!("Not in an Activity context")),
    };

    let children = tracker
        .get_child_work_items(&wi_id, Some(tracker.todo_wi_type()))
        .await?;

    let in_progress = tracker.todo_in_progress_status();
    let done = tracker.todo_complete_status();

    println!("## Todos for WI #{}", wi_id);
    for child in children {
        if !all && (child.state == "Closed" || child.state == done) {
            continue;
        }
        let icon = if child.state == in_progress {
            "●"
        } else if child.state == "Closed" || child.state == done {
            "✓"
        } else {
            "○"
        };
        println!(
            "  {}  #{}  {}  ({})",
            icon, child.id, child.title, child.state
        );
        if detail {
            if let Some(desc) = child.description {
                println!("             {}", desc);
            }
        }
    }

    Ok(())
}

pub async fn new(
    title: String,
    description: Option<String>,
    assigned_to: Option<String>,
    pick: bool,
) -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let tracker = provider_set.issue_tracker;

    if !tracker.capabilities().work_item_hierarchy {
        return Err(anyhow!(
            "Work item hierarchy is not supported by this provider. Todo commands are unavailable."
        ));
    }

    let git = LocalGitProvider;
    let branch = git.get_current_branch().await?;

    let wi_id = match ContextManager::detect(&branch) {
        Context::Activity { wi_id, .. } => wi_id,
        _ => return Err(anyhow!("Not in an Activity context")),
    };

    let task = tracker
        .create_work_item(
            &title,
            tracker.todo_wi_type(),
            description.as_deref(),
            assigned_to.as_deref(),
            None,
        )
        .await?;
    tracker
        .link_work_items(&wi_id, &task.id, "System.LinkTypes.Hierarchy-Forward")
        .await?;

    if pick {
        tracker
            .update_work_item_state(&task.id, tracker.todo_in_progress_status())
            .await?;
    }

    println!("Todo #{} created and linked to WI #{}.", task.id, wi_id);
    Ok(())
}

async fn resolve_ref(
    tracker: &dyn IssueTracker,
    wi_id: &WorkItemId,
    reference: &str,
) -> Result<WorkItemId> {
    if reference.chars().all(|c| c.is_numeric()) {
        return Ok(WorkItemId::from(reference));
    }

    let children = tracker
        .get_child_work_items(wi_id, Some(tracker.todo_wi_type()))
        .await?;
    let matches: Vec<_> = children
        .into_iter()
        .filter(|c| c.title.to_lowercase().contains(&reference.to_lowercase()))
        .collect();

    if matches.is_empty() {
        return Err(anyhow!("No todo matching `{}` found.", reference));
    }
    if matches.len() > 1 {
        println!("Multiple matches found:");
        for m in &matches {
            println!("  #{}  {}", m.id, m.title);
        }
        return Err(anyhow!("Ambiguous reference `{}`", reference));
    }

    Ok(matches[0].id.clone())
}

pub async fn pick(reference: String) -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let tracker = provider_set.issue_tracker;

    if !tracker.capabilities().work_item_hierarchy {
        return Err(anyhow!(
            "Work item hierarchy is not supported by this provider. Todo commands are unavailable."
        ));
    }

    let git = LocalGitProvider;
    let branch = git.get_current_branch().await?;

    let wi_id = match ContextManager::detect(&branch) {
        Context::Activity { wi_id, .. } => wi_id,
        _ => return Err(anyhow!("Not in an Activity context")),
    };

    let task_id = resolve_ref(tracker.as_ref(), &wi_id, &reference).await?;
    tracker
        .update_work_item_state(&task_id, tracker.todo_in_progress_status())
        .await?;

    println!(
        "Todo #{} is now {}.",
        task_id,
        tracker.todo_in_progress_status()
    );
    Ok(())
}

pub async fn complete(reference: String) -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let tracker = provider_set.issue_tracker;

    if !tracker.capabilities().work_item_hierarchy {
        return Err(anyhow!(
            "Work item hierarchy is not supported by this provider. Todo commands are unavailable."
        ));
    }

    let git = LocalGitProvider;
    let branch = git.get_current_branch().await?;

    let wi_id = match ContextManager::detect(&branch) {
        Context::Activity { wi_id, .. } => wi_id,
        _ => return Err(anyhow!("Not in an Activity context")),
    };

    let task_id = resolve_ref(tracker.as_ref(), &wi_id, &reference).await?;
    tracker.update_work_item_state(&task_id, "Closed").await?;

    println!("Todo #{} is now Closed.", task_id);
    Ok(())
}

pub async fn reopen(reference: String) -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let tracker = provider_set.issue_tracker;

    if !tracker.capabilities().work_item_hierarchy {
        return Err(anyhow!(
            "Work item hierarchy is not supported by this provider. Todo commands are unavailable."
        ));
    }

    let git = LocalGitProvider;
    let branch = git.get_current_branch().await?;

    let wi_id = match ContextManager::detect(&branch) {
        Context::Activity { wi_id, .. } => wi_id,
        _ => return Err(anyhow!("Not in an Activity context")),
    };

    let task_id = resolve_ref(tracker.as_ref(), &wi_id, &reference).await?;
    tracker.update_work_item_state(&task_id, "New").await?;

    println!("Todo #{} is now New.", task_id);
    Ok(())
}

pub async fn update(
    reference: String,
    title: Option<String>,
    description: Option<String>,
    assigned_to: Option<String>,
    state: Option<String>,
) -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let tracker = provider_set.issue_tracker;
    let git = LocalGitProvider;
    let branch = git.get_current_branch().await?;

    let wi_id = match ContextManager::detect(&branch) {
        Context::Activity { wi_id, .. } => wi_id,
        _ => return Err(anyhow!("Not in an Activity context")),
    };

    let task_id = resolve_ref(tracker.as_ref(), &wi_id, &reference).await?;
    tracker
        .update_work_item(
            &task_id,
            title.as_deref(),
            description.as_deref(),
            assigned_to.as_deref(),
            None,
        )
        .await?;

    if let Some(s) = state {
        tracker.update_work_item_state(&task_id, &s).await?;
    }

    println!("Todo #{} updated.", task_id);
    Ok(())
}

pub async fn next(pick_it: bool) -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let tracker = provider_set.issue_tracker;
    let git = LocalGitProvider;
    let branch = git.get_current_branch().await?;

    let wi_id = match ContextManager::detect(&branch) {
        Context::Activity { wi_id, .. } => wi_id,
        _ => return Err(anyhow!("Not in an Activity context")),
    };

    let children = tracker
        .get_child_work_items(&wi_id, Some(tracker.todo_wi_type()))
        .await?;
    let next_task = children
        .into_iter()
        .filter(|c| c.state == "New")
        .min_by_key(|c| c.id.clone());

    if let Some(task) = next_task {
        println!("Next Todo: #{} {}", task.id, task.title);
        if pick_it {
            tracker
                .update_work_item_state(&task.id, tracker.todo_in_progress_status())
                .await?;
            println!("Todo #{} is now Active.", task.id);
        }
    } else {
        println!("No more New todos.");
    }

    Ok(())
}
