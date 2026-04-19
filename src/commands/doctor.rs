use crate::core::config::Config;
use crate::providers::adonet::AzureDevOpsProvider;
use crate::providers::git::LocalGitProvider;
use crate::providers::sonar::SonarProvider;
use crate::providers::{IssueTracker, VCSProvider};
use anyhow::Result;
use std::process::Command;

pub async fn run(fix: bool) -> Result<()> {
    let config = Config::load()?;
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
    if let Some(ado_config) = config.ado_config() {
        let ado = AzureDevOpsProvider::new(ado_config)?;
        let ado_check = ado.get_repository(&ado_config.project).await.is_ok();
        println!("| ADO   | {} |", if ado_check { "✓" } else { "✗" });

        if let Some(sonar_config) = &config.sonar {
            let _sonar = SonarProvider::new(sonar_config)?;
            println!("| Sonar | ✓ |");
        }

        // 3. Check Submodules
        let git = LocalGitProvider;
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
            let branch = git.get_current_branch().await?;
            let context = crate::core::context::ContextManager::detect(&branch);

            if let crate::core::context::Context::Activity { wi_id, .. } = context {
                println!("Detected activity for WI #{}", wi_id);
                ado.update_work_item_state(&wi_id, "Active").await?;
                println!("- WI state set to Active");
            }
        }
    } else {
        println!("| Provider | ✗ (no ADO config) |");
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
