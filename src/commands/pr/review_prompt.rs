use crate::commands::pr::feedback;
use crate::core::config::Config;
use crate::core::context::{Context, ContextManager};
use crate::providers::factory::ProviderSet;
use crate::providers::git::LocalGitProvider;
use crate::providers::VCSProvider;
use anyhow::{anyhow, Result};
use std::fmt::Write as FmtWrite;
use std::io::Read as IoRead;

const SAMPLE_TEMPLATE: &str = r#"You are a senior software engineer reviewing a pull request. Use the information below to write a structured code review.

## Task Context
- Task ID: {{TASK_ID}}
- Task Title: {{TASK_TITLE}}
- Task Description:
{{TASK_DESCRIPTION}}

## Pull Request Context
- PR ID: {{PR_ID}}
- PR Title: {{PR_TITLE}}
- Status: {{PR_STATUS}}
- Author: {{PR_AUTHOR}}
- Source Branch: {{PR_SOURCE_BRANCH}}
- Target Branch: {{PR_TARGET_BRANCH}}
- Repository: {{REPO_NAME}}
- Date: {{TODAY}}

## Changed Files
{{PR_CHANGED_FILES}}

## Full Task Details
{{TASK_TEXT}}

## Full PR Details (threads, description, diff summary)
{{PR_TEXT_FULL}}

---

Produce a review using the format described below.

## Review Format Reference
{{FEEDBACK_STRUCTURE}}

## Output Schema (YAML)
```yaml
{{FEEDBACK_SCHEMA_YAML}}
```
"#;

const PLACEHOLDER_DOCS: &str = r#"Available placeholders (wrap in {{ }} in your template):

  PR_ID              Pull request number
  PR_TITLE           Pull request title
  PR_STATUS          PR status: active, draft, completed, abandoned
  PR_AUTHOR          PR author display name
  PR_SOURCE_BRANCH   Source (feature) branch name
  PR_TARGET_BRANCH   Target (base) branch name
  PR_CHANGED_FILES   Markdown table of changed files and change types
  PR_TEXT_FULL       Full PR document: description, threads, changed files
                     (respects --show-closed-threads flag)

  TASK_ID            Work item / issue ID linked to the current branch
  TASK_TITLE         Work item title
  TASK_DESCRIPTION   Work item description (raw text)
  TASK_TEXT          Full task output (id, state, title, description, comments)

  FEEDBACK_STRUCTURE    Output of `fm pr feedback structure` (format reference)
  FEEDBACK_SCHEMA_YAML  YAML schema for review.yaml
  FEEDBACK_SCHEMA_JSON  JSON schema for review.yaml

  REPO_NAME          Repository name
  TODAY              Current date (YYYY-MM-DD)
"#;

pub async fn run(
    pr: Option<String>,
    text: Option<String>,
    show_closed_threads: bool,
    sample: bool,
) -> Result<()> {
    if sample {
        println!("{}", SAMPLE_TEMPLATE);
        eprintln!("{}", PLACEHOLDER_DOCS);
        return Ok(());
    }

    let template = match text {
        Some(t) => t,
        None => {
            let mut buf = String::new();
            std::io::stdin()
                .read_to_string(&mut buf)
                .map_err(|e| anyhow!("Failed to read stdin: {}", e))?;
            if buf.trim().is_empty() {
                return Err(anyhow!(
                    "No template provided. Use --text or pipe a template via stdin. \
                     Use --sample to see an example."
                ));
            }
            buf
        }
    };

    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let tracker = provider_set.issue_tracker;
    let vcs = provider_set.vcs;
    let git = LocalGitProvider;
    let repo_name = git.get_repo_name()?;

    let pr_id = super::resolve_pr_id(pr, vcs.as_ref(), tracker.as_ref(), &git, &repo_name).await?;
    let pr_details = vcs.get_pull_request_details(&repo_name, &pr_id).await?;

    let branch = git.get_current_branch().await?;
    let wi_id = match ContextManager::detect(&branch) {
        Context::Activity { wi_id, .. } => Some(wi_id),
        _ => None,
    };

    let wi = if let Some(ref id) = wi_id {
        tracker.get_work_item(id).await.ok()
    } else {
        None
    };

    let changed_files = vcs
        .get_pull_request_changed_files(&repo_name, &pr_id)
        .await
        .unwrap_or_default();

    let pr_text_full =
        super::build_pr_doc(&pr_id, vcs.as_ref(), &repo_name, show_closed_threads, false).await?;

    let task_text = if let Some(ref wi) = wi {
        let comments = tracker
            .get_work_item_comments(&wi.id)
            .await
            .unwrap_or_default();
        let mut t = String::new();
        writeln!(t, "## {} [{}] - {}", wi.id, wi.state, wi.title).unwrap();
        if let Some(pr_ref) = pr_details.source_branch.strip_prefix("refs/heads/") {
            writeln!(t, "\nBranch: {}", pr_ref).unwrap();
        }
        if let Some(desc) = &wi.description {
            if !desc.is_empty() {
                writeln!(t, "\n{}", desc).unwrap();
            }
        }
        for comment in &comments {
            let date = comment.created_at_date.as_deref().unwrap_or("");
            let time = comment.created_at_time.as_deref().unwrap_or("");
            writeln!(t, "\n### {} {} - {}", date, time, comment.author).unwrap();
            writeln!(t, "\n{}", comment.text).unwrap();
        }
        t
    } else {
        String::new()
    };

    let mut files_table = String::new();
    writeln!(files_table, "| File | Change |").unwrap();
    writeln!(files_table, "|---|---|").unwrap();
    for f in &changed_files {
        writeln!(files_table, "| {} | {} |", f.path, f.change_type).unwrap();
    }

    let today = {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let days = secs / 86400;
        // Julian Day Number to Gregorian calendar
        let j = days as i64 + 2440588;
        let a = j + 32044;
        let b = (4 * a + 3) / 146097;
        let c = a - (b * 146097) / 4;
        let d = (4 * c + 3) / 1461;
        let e = c - (1461 * d) / 4;
        let m = (5 * e + 2) / 153;
        let day = e - (153 * m + 2) / 5 + 1;
        let month = m + 3 - 12 * (m / 10);
        let year = b * 100 + d - 4800 + m / 10;
        format!("{:04}-{:02}-{:02}", year, month, day)
    };

    let source = pr_details.source_branch.replace("refs/heads/", "");
    let target = pr_details.target_branch.replace("refs/heads/", "");

    let result = template
        .replace("{{PR_ID}}", &pr_id)
        .replace("{{PR_TITLE}}", &pr_details.title)
        .replace("{{PR_STATUS}}", &pr_details.status)
        .replace(
            "{{PR_AUTHOR}}",
            pr_details.author.as_deref().unwrap_or("unknown"),
        )
        .replace("{{PR_SOURCE_BRANCH}}", &source)
        .replace("{{PR_TARGET_BRANCH}}", &target)
        .replace("{{PR_CHANGED_FILES}}", &files_table)
        .replace("{{PR_TEXT_FULL}}", &pr_text_full)
        .replace(
            "{{TASK_ID}}",
            wi.as_ref().map(|w| w.id.as_str()).unwrap_or(""),
        )
        .replace(
            "{{TASK_TITLE}}",
            wi.as_ref().map(|w| w.title.as_str()).unwrap_or(""),
        )
        .replace(
            "{{TASK_DESCRIPTION}}",
            wi.as_ref()
                .and_then(|w| w.description.as_deref())
                .unwrap_or(""),
        )
        .replace("{{TASK_TEXT}}", &task_text)
        .replace("{{REPO_NAME}}", &repo_name)
        .replace("{{TODAY}}", &today)
        .replace("{{FEEDBACK_STRUCTURE}}", feedback::structure_text())
        .replace("{{FEEDBACK_SCHEMA_YAML}}", feedback::schema_yaml_text())
        .replace("{{FEEDBACK_SCHEMA_JSON}}", feedback::schema_json_text());

    print!("{}", result);
    Ok(())
}
