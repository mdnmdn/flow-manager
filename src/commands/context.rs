use crate::core::config::Config;
use crate::core::context::{Context, ContextManager};
use crate::providers::factory::ProviderSet;
use crate::providers::git::LocalGitProvider;
use crate::providers::VCSProvider;
use anyhow::Result;
use tokio::join;

pub async fn run(only_wi: bool, only_pr: bool, only_git: bool, only_pipeline: bool) -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let tracker = provider_set.issue_tracker;
    let vcs = provider_set.vcs;
    let pipeline = provider_set.pipeline;
    let git = LocalGitProvider;
    let branch = git.get_current_branch().await?;
    let context = ContextManager::detect(&branch);

    match context {
        Context::Baseline { branch } => {
            println!("## Context — `{}` (baseline)", branch);
            let log = git.get_log(None, Some(5)).await?;
            println!("\nLast commits:\n{}", log);
        }
        Context::Activity { branch, wi_id, .. } => {
            let repo_name = config.fm.submodules.first().cloned().unwrap_or_default();
            // Parallelize fetches
            let wi_fut = tracker.get_work_item(&wi_id);
            let pr_fut = vcs.get_pull_request_by_branch(&repo_name, &branch);
            let git_status_fut = git.get_status();
            let target = format!("origin/{}", config.fm.default_target);
            let ahead_range = format!("{}..HEAD", target);
            let behind_range = format!("HEAD..{}", target);
            let log_ahead_fut = git.get_log(Some(&ahead_range), None);
            let log_behind_fut = git.get_log(Some(&behind_range), None);
            let pipeline_fut = async {
                if let Some(p) = &pipeline {
                    p.get_latest_run(&branch).await
                } else {
                    Ok(None)
                }
            };

            let (wi_res, pr_res, git_res, ahead_res, behind_res, pipe_res) = join!(
                wi_fut,
                pr_fut,
                git_status_fut,
                log_ahead_fut,
                log_behind_fut,
                pipeline_fut
            );

            let wi = wi_res.ok();
            let pr = pr_res.unwrap_or(None);
            let git_status = git_res.unwrap_or_default();
            let ahead = ahead_res.unwrap_or_default();
            let behind = behind_res.unwrap_or_default();
            let pipe = pipe_res.unwrap_or(None);

            if only_wi {
                if let Some(w) = &wi {
                    println!("### Work Item\n| | |\n|-|---|\n| ID | #{} |\n| Title | {} |\n| State | {} |", w.id, w.title, w.state);
                }
            } else if only_pr {
                if let Some(p) = &pr {
                    println!("### Pull Request\n| | |\n|-|---|\n| ID | #{} |\n| Title | {} |\n| State | {} |", p.id, p.title, p.status);
                }
            } else if only_git {
                println!("### Git Status\n{}", git_status);
            } else if only_pipeline {
                if let Some(p) = &pipe {
                    println!("### CI Pipeline\n| | |\n|-|---|\n| ID | #{} |\n| Status | {} |\n| Result | {:?} |", p.id, p.status, p.result);
                }
            } else {
                println!("## Context — `{}`", branch);
                if let Some(w) = &wi {
                    println!("\n### Work Item\n- #{}: {} ({})", w.id, w.title, w.state);
                }
                if let Some(p) = &pr {
                    println!(
                        "\n### Pull Request\n- #{}: {} ({})",
                        p.id, p.title, p.status
                    );
                }
                println!(
                    "\n### Git Status\n{}",
                    if git_status.is_empty() {
                        "Clean"
                    } else {
                        &git_status
                    }
                );
                let ahead_count = ahead.lines().count();
                let behind_count = behind.lines().count();
                println!("- Ahead: {}, Behind: {}", ahead_count, behind_count);
                if let Some(p) = &pipe {
                    println!(
                        "\n### CI Pipeline\n- #{}: {} ({:?})",
                        p.id, p.status, p.result
                    );
                }
            }
        }
    }

    Ok(())
}
