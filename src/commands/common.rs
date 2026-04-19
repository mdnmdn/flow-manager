use crate::core::context::{Context, ContextManager};
use crate::core::models::WorkItemId;
use crate::providers::git::LocalGitProvider;
use crate::providers::VCSProvider;
use anyhow::Result;

/// If the current branch is an activity branch with uncommitted changes, stash them
/// into two named stashes (`stash-{wi-id}-staged` / `stash-{wi-id}-unstaged`) and
/// push the branch. Returns the wi_id of the stashed activity, or `None` if we were
/// already on a clean branch or on baseline.
pub async fn stash_and_push_current_activity(git: &LocalGitProvider) -> Result<Option<WorkItemId>> {
    let branch = git.get_current_branch().await?;
    let wi_id = match ContextManager::detect(&branch) {
        Context::Activity { wi_id, .. } => wi_id,
        Context::Baseline { .. } => return Ok(None),
    };

    let status = git.get_status().await?;
    if !status.is_empty() {
        let stash_base = format!("stash-{}-", wi_id);
        if git.has_staged_changes()? {
            git.stash_push_staged(&format!("{}staged", stash_base))
                .await?;
        }
        let remaining = git.get_status().await?;
        if !remaining.is_empty() {
            git.stash_push(&format!("{}unstaged", stash_base)).await?;
        }
        println!("Stashed changes for activity #{}.", wi_id);
    }

    git.push(false).await?;
    Ok(Some(wi_id))
}
