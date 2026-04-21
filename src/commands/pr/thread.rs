use crate::core::config::Config;
use crate::providers::factory::ProviderSet;
use crate::providers::git::LocalGitProvider;
use anyhow::Result;
use std::io::Read;

pub async fn list(id: Option<String>, status_filter: String) -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let tracker = provider_set.issue_tracker;
    let vcs = provider_set.vcs;
    let git = LocalGitProvider;
    let repo_name = git.get_repo_name()?;

    let pr_id = super::resolve_pr_id(id, vcs.as_ref(), tracker.as_ref(), &git, &repo_name).await?;

    let threads = vcs.get_pull_request_threads(&repo_name, &pr_id).await?;

    let filtered: Vec<_> = threads
        .iter()
        .filter(|t| match status_filter.as_str() {
            "active" => t.status == "active",
            "resolved" => t.status != "active",
            _ => true,
        })
        .collect();

    if filtered.is_empty() {
        println!("No threads found.");
        return Ok(());
    }

    println!(
        "{:<6} {:<10} {:<35} {:<6} {:<15} PREVIEW",
        "ID", "STATUS", "FILE", "LINE", "AUTHOR"
    );
    println!("{}", "-".repeat(100));

    for t in filtered {
        let file = t
            .file_path
            .as_deref()
            .unwrap_or("")
            .trim_start_matches('/')
            .chars()
            .take(35)
            .collect::<String>();
        let line = t.line.map(|l| l.to_string()).unwrap_or_default();
        let author: String = t.author.chars().take(15).collect();
        let preview: String = t
            .content
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(50)
            .collect();

        println!(
            "{:<6} {:<10} {:<35} {:<6} {:<15} {}",
            t.id, t.status, file, line, author, preview
        );
    }

    Ok(())
}

pub async fn reply(
    pr_id: Option<String>,
    thread_id: String,
    message: String,
    resolve: bool,
) -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let tracker = provider_set.issue_tracker;
    let vcs = provider_set.vcs;
    let git = LocalGitProvider;
    let repo_name = git.get_repo_name()?;

    let resolved_pr_id =
        super::resolve_pr_id(pr_id, vcs.as_ref(), tracker.as_ref(), &git, &repo_name).await?;

    let msg = if message == "-" {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf.trim_end().to_string()
    } else {
        message
    };

    vcs.reply_to_pull_request_thread(&repo_name, &resolved_pr_id, &thread_id, &msg)
        .await?;
    println!("Reply posted to thread {}.", thread_id);

    if resolve {
        vcs.update_pull_request_thread_status(&repo_name, &resolved_pr_id, &thread_id, "fixed")
            .await?;
        println!("Thread {} resolved.", thread_id);
    }

    Ok(())
}

pub async fn resolve(
    pr_id: Option<String>,
    thread_ids: Vec<String>,
    comment: Option<String>,
) -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let tracker = provider_set.issue_tracker;
    let vcs = provider_set.vcs;
    let git = LocalGitProvider;
    let repo_name = git.get_repo_name()?;

    let resolved_pr_id =
        super::resolve_pr_id(pr_id, vcs.as_ref(), tracker.as_ref(), &git, &repo_name).await?;

    for thread_id in &thread_ids {
        if let Some(msg) = &comment {
            vcs.reply_to_pull_request_thread(&repo_name, &resolved_pr_id, thread_id, msg)
                .await?;
        }
        vcs.update_pull_request_thread_status(&repo_name, &resolved_pr_id, thread_id, "fixed")
            .await?;
        println!("Thread {} resolved.", thread_id);
    }

    Ok(())
}
