use crate::core::config::Config;
use crate::providers::factory::ProviderSet;
use crate::providers::git::LocalGitProvider;
use crate::providers::sonar::SonarProvider;
use crate::providers::VCSProvider;
use anyhow::Result;
use std::process::Command;

pub async fn run(fix: bool) -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let tracker = provider_set.issue_tracker;
    let vcs = provider_set.vcs;

    println!("## Flow Manager Doctor\n");

    // 1. Check Git
    let git_check = check_git();
    let git_repo_check = check_git_repo();
    println!("| Check | Status |");
    println!("|-------|--------|");
    println!("| Git Installed | {} |", if git_check { "✓" } else { "✗" });
    println!(
        "| Git Repo      | {} |",
        if git_repo_check { "✓" } else { "✗" }
    );

    // 2. Check Providers
    let git = LocalGitProvider;
    let repo_name = git.get_repo_name().unwrap_or_default();
    let provider_check = vcs.get_repository(&repo_name).await.is_ok();
    println!("| Provider | {} |", if provider_check { "✓" } else { "✗" });

    if let Some(sonar_config) = &config.sonar {
        let _sonar = SonarProvider::new(sonar_config)?;
        // Simple ping or list projects if possible, for now just check if we can build it
        println!("| Sonar | ✓ |");
    }

    // 3. Check Submodules
    for sub in &config.fm.submodules {
        let exists = std::path::Path::new(sub).exists();
        println!(
            "| Submodule `{}` | {} |",
            sub,
            if exists { "✓" } else { "✗" }
        );
    }

    if fix {
        println!("\n### Fixing invariants...");
        // Implement fix logic: repair links, etc.
        // This requires detecting current context
        let branch = git.get_current_branch().await?;
        let context = crate::core::context::ContextManager::detect(&branch);

        if let crate::core::context::Context::Activity {
            branch: _,
            wi_id,
            wi_type: _,
        } = context
        {
            println!("Detected activity for WI #{}", wi_id);
            // 1. Ensure WI is Active
            tracker.update_work_item_state(&wi_id, "Active").await?;

            // 2. Ensure Branch and PR links exist
            // (Logic to fetch branch URL and PR URL and call ado.create_artifact_link)
            // For now, this is a placeholder for the actual repair logic
            println!("- WI state set to Active");
        }
    }

    Ok(())
}

fn check_git() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

fn check_git_repo() -> bool {
    Command::new("git")
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
