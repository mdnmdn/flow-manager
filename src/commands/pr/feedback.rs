use crate::core::config::Config;
use crate::core::models::{ChangedFile, PullRequestThread, ReviewFile, ReviewNewThread};
use crate::providers::factory::ProviderSet;
use crate::providers::git::LocalGitProvider;
use anyhow::{anyhow, Result};
use regex::Regex;

pub fn structure() -> Result<()> {
    println!(
        r#"# review.yaml — structure

A review file has two required fields and three optional arrays.

## Required

  summary: string (≥10 chars)
    Free-text summary of the review. Posted as the top-level PR comment.

  recommendation: approve | request_changes | needs_discussion
    Overall verdict.

## Optional arrays

  threads[]          — actions on existing PR threads
    id:      integer   thread ID (must exist in the PR)
    action:  resolve | reply
    comment: string    text posted before resolving, or as a reply

  new_threads[]      — new file-anchored comments to create
    file:     string   path relative to repo root
    line:     integer  target line number (≥1)
    severity: critical | major | minor | positive
    comment:  string

  open_points[]      — status of each PR description checklist item
    ref:     string    must match (case-insensitive substring) an open point from the PR description
    status:  addressed | not_addressed | partially_addressed
    comment: string

## Notes

- All comment fields are plain text; no Markdown rendering is assumed by the CLI.
- Threads whose status is not "active" are skipped during apply (warned during validate).
- new_threads outside the PR diff are allowed but produce a validate warning.
- open_points refs that do not match any PR checklist item are a hard error.

## review.md (alternative format)

Wrap each action in a fenced block tagged  ```action:<type>```.
Prose outside blocks becomes the summary. End with:
  **Recommendation:** <value>

Block types: thread, new_thread, open_point
Each block body is parsed as inline YAML with the same fields as above."#
    );
    Ok(())
}

pub fn schema() -> Result<()> {
    println!("# --- JSON Schema ---\n");
    println!(
        r#"{{
  "$schema": "http://json-schema.org/draft-07/schema#",
  "type": "object",
  "required": ["summary", "recommendation"],
  "additionalProperties": false,
  "properties": {{
    "summary": {{ "type": "string", "minLength": 10 }},
    "recommendation": {{
      "type": "string",
      "enum": ["approve", "request_changes", "needs_discussion"]
    }},
    "threads": {{
      "type": "array",
      "items": {{
        "type": "object",
        "required": ["id", "action", "comment"],
        "additionalProperties": false,
        "properties": {{
          "id": {{ "type": "integer" }},
          "action": {{ "type": "string", "enum": ["resolve", "reply"] }},
          "comment": {{ "type": "string", "minLength": 1 }}
        }}
      }}
    }},
    "new_threads": {{
      "type": "array",
      "items": {{
        "type": "object",
        "required": ["file", "line", "severity", "comment"],
        "additionalProperties": false,
        "properties": {{
          "file": {{ "type": "string" }},
          "line": {{ "type": "integer", "minimum": 1 }},
          "severity": {{
            "type": "string",
            "enum": ["critical", "major", "minor", "positive"]
          }},
          "comment": {{ "type": "string", "minLength": 1 }}
        }}
      }}
    }},
    "open_points": {{
      "type": "array",
      "items": {{
        "type": "object",
        "required": ["ref", "status", "comment"],
        "additionalProperties": false,
        "properties": {{
          "ref": {{ "type": "string" }},
          "status": {{
            "type": "string",
            "enum": ["addressed", "not_addressed", "partially_addressed"]
          }},
          "comment": {{ "type": "string" }}
        }}
      }}
    }}
  }}
}}"#
    );

    println!("\n# --- YAML Schema ---\n");
    println!(
        r#"$schema: "http://json-schema.org/draft-07/schema#"
type: object
required:
  - summary
  - recommendation
additionalProperties: false
properties:
  summary:
    type: string
    minLength: 10
  recommendation:
    type: string
    enum:
      - approve
      - request_changes
      - needs_discussion
  threads:
    type: array
    items:
      type: object
      required: [id, action, comment]
      additionalProperties: false
      properties:
        id:
          type: integer
        action:
          type: string
          enum: [resolve, reply]
        comment:
          type: string
          minLength: 1
  new_threads:
    type: array
    items:
      type: object
      required: [file, line, severity, comment]
      additionalProperties: false
      properties:
        file:
          type: string
        line:
          type: integer
          minimum: 1
        severity:
          type: string
          enum: [critical, major, minor, positive]
        comment:
          type: string
          minLength: 1
  open_points:
    type: array
    items:
      type: object
      required: [ref, status, comment]
      additionalProperties: false
      properties:
        ref:
          type: string
        status:
          type: string
          enum: [addressed, not_addressed, partially_addressed]
        comment:
          type: string"#
    );
    Ok(())
}

fn detect_format(path: &str, explicit: Option<&str>) -> &'static str {
    match explicit {
        Some(f) if f.starts_with("md") => "md",
        Some(_) => "yaml",
        None => {
            if path.ends_with(".md") {
                "md"
            } else {
                "yaml"
            }
        }
    }
}

fn parse_yaml(path: &str) -> Result<ReviewFile> {
    let content =
        std::fs::read_to_string(path).map_err(|e| anyhow!("Cannot read {}: {}", path, e))?;
    serde_yaml::from_str(&content).map_err(|e| anyhow!("Failed to parse YAML: {}", e))
}

fn parse_md(path: &str) -> Result<ReviewFile> {
    let content =
        std::fs::read_to_string(path).map_err(|e| anyhow!("Cannot read {}: {}", path, e))?;

    let block_re = Regex::new(r"(?s)```action:(\w+)\n(.*?)```").unwrap();
    let rec_re = Regex::new(r"(?i)\*\*Recommendation:\*\*\s*(\S+)").unwrap();

    let mut threads = vec![];
    let mut new_threads = vec![];
    let mut open_points = vec![];
    let mut recommendation = String::new();
    let mut prose_parts: Vec<String> = vec![];

    let mut last_end = 0usize;
    for cap in block_re.captures_iter(&content) {
        let full_match = cap.get(0).unwrap();
        let block_type = cap.get(1).unwrap().as_str();
        let block_content = cap.get(2).unwrap().as_str();

        let before = &content[last_end..full_match.start()];
        let prose_trimmed = before.trim();
        if !prose_trimmed.is_empty() {
            prose_parts.push(prose_trimmed.to_string());
        }
        last_end = full_match.end();

        match block_type {
            "thread" => {
                let action: serde_yaml::Value = serde_yaml::from_str(block_content)
                    .map_err(|e| anyhow!("Bad thread block: {}", e))?;
                threads.push(crate::core::models::ReviewThreadAction {
                    id: action["id"]
                        .as_u64()
                        .ok_or_else(|| anyhow!("thread block missing id"))?,
                    action: action["action"].as_str().unwrap_or("reply").to_string(),
                    comment: action["comment"].as_str().unwrap_or("").to_string(),
                });
            }
            "new_thread" => {
                let t: serde_yaml::Value = serde_yaml::from_str(block_content)
                    .map_err(|e| anyhow!("Bad new_thread block: {}", e))?;
                new_threads.push(ReviewNewThread {
                    file: t["file"].as_str().unwrap_or("").to_string(),
                    line: t["line"].as_u64().unwrap_or(1) as u32,
                    severity: t["severity"].as_str().unwrap_or("minor").to_string(),
                    comment: t["comment"].as_str().unwrap_or("").to_string(),
                });
            }
            "open_point" => {
                let op: serde_yaml::Value = serde_yaml::from_str(block_content)
                    .map_err(|e| anyhow!("Bad open_point block: {}", e))?;
                open_points.push(crate::core::models::ReviewOpenPoint {
                    ref_: op["ref"].as_str().unwrap_or("").to_string(),
                    status: op["status"].as_str().unwrap_or("not_addressed").to_string(),
                    comment: op["comment"].as_str().unwrap_or("").to_string(),
                });
            }
            _ => {}
        }
    }

    let tail = content[last_end..].trim();
    if !tail.is_empty() {
        prose_parts.push(tail.to_string());
    }

    if let Some(cap) = rec_re.captures(&content) {
        recommendation = cap.get(1).unwrap().as_str().to_string();
    }

    let summary = prose_parts
        .iter()
        .map(|p| {
            p.lines()
                .filter(|l| !l.starts_with('#') && !l.starts_with("**Recommendation:"))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
        .trim()
        .to_string();

    Ok(ReviewFile {
        summary,
        recommendation,
        threads,
        new_threads,
        open_points,
    })
}

fn validate_review(
    review: &ReviewFile,
    live_threads: &[PullRequestThread],
    changed_files: &[ChangedFile],
    open_points_ctx: &[String],
) -> (Vec<String>, Vec<String>) {
    let mut errors = vec![];
    let mut warnings = vec![];

    if review.summary.trim().len() < 10 {
        errors.push("summary must be at least 10 characters".to_string());
    }

    let valid_rec = ["approve", "request_changes", "needs_discussion"];
    if !valid_rec.contains(&review.recommendation.as_str()) {
        errors.push(format!(
            "recommendation '{}' is invalid — must be one of: {}",
            review.recommendation,
            valid_rec.join(", ")
        ));
    }

    let valid_actions = ["resolve", "reply"];
    for ta in &review.threads {
        if !valid_actions.contains(&ta.action.as_str()) {
            errors.push(format!(
                "thread {} action '{}' is invalid — must be resolve or reply",
                ta.id, ta.action
            ));
        }
        let live = live_threads.iter().find(|t| t.id == ta.id.to_string());
        match live {
            None => errors.push(format!("thread {} does not exist in this PR", ta.id)),
            Some(t) if t.status != "active" => {
                warnings.push(format!(
                    "thread {} is already {} — action will be skipped",
                    ta.id, t.status
                ));
            }
            _ => {}
        }
    }

    let valid_sev = ["critical", "major", "minor", "positive"];
    for nt in &review.new_threads {
        if !valid_sev.contains(&nt.severity.as_str()) {
            errors.push(format!(
                "new_thread on {} severity '{}' is invalid — must be one of: {}",
                nt.file,
                nt.severity,
                valid_sev.join(", ")
            ));
        }
        let norm = nt.file.trim_start_matches('/');
        let exists = changed_files
            .iter()
            .any(|f| f.path.trim_start_matches('/') == norm);
        if !exists {
            warnings.push(format!(
                "{} is outside the PR diff — thread will still be posted",
                nt.file
            ));
        }
    }

    let valid_status = ["addressed", "not_addressed", "partially_addressed"];
    for op in &review.open_points {
        if !valid_status.contains(&op.status.as_str()) {
            errors.push(format!(
                "open_point '{}' status '{}' is invalid — must be one of: {}",
                op.ref_,
                op.status,
                valid_status.join(", ")
            ));
        }
        let matched = open_points_ctx
            .iter()
            .any(|p| p.to_lowercase().contains(&op.ref_.to_lowercase()));
        if !matched {
            errors.push(format!(
                "open_point ref '{}' not found in PR open points",
                op.ref_
            ));
        }
    }

    (errors, warnings)
}

pub async fn validate(file: String, pr_id: Option<String>, format: Option<String>) -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let tracker = provider_set.issue_tracker;
    let vcs = provider_set.vcs;
    let git = LocalGitProvider;
    let repo_name = git.get_repo_name()?;

    let resolved_pr_id =
        super::resolve_pr_id(pr_id, vcs.as_ref(), tracker.as_ref(), &git, &repo_name).await?;

    let fmt = detect_format(&file, format.as_deref());
    let review = if fmt == "md" {
        parse_md(&file)?
    } else {
        parse_yaml(&file)?
    };

    println!("Validating {} against PR #{}…\n", file, resolved_pr_id);

    println!("Schema");
    if fmt == "yaml" {
        println!("  ✅ Valid YAML");
    } else {
        println!("  ✅ Valid MD");
    }
    println!("  ✅ Required fields present (summary, recommendation)");
    println!("  ✅ recommendation value: {}", review.recommendation);
    println!();

    let threads = vcs
        .get_pull_request_threads(&repo_name, &resolved_pr_id)
        .await
        .unwrap_or_default();
    let changed_files = vcs
        .get_pull_request_changed_files(&repo_name, &resolved_pr_id)
        .await
        .unwrap_or_default();
    let pr = vcs
        .get_pull_request_details(&repo_name, &resolved_pr_id)
        .await?;
    let open_points_ctx = super::extract_open_points(pr.description.as_deref().unwrap_or(""));

    let (errors, warnings) = validate_review(&review, &threads, &changed_files, &open_points_ctx);

    if !review.threads.is_empty() {
        println!("Threads");
        for ta in &review.threads {
            let live = threads.iter().find(|t| t.id == ta.id.to_string());
            match live {
                None => println!("  ❌ Thread {} does not exist in this PR", ta.id),
                Some(t) if t.status != "active" => println!(
                    "  ⚠️  Thread {} exists but is already {} → action will be skipped",
                    ta.id, t.status
                ),
                _ => println!(
                    "  ✅ Thread {} exists · status: active → action: {} ✓",
                    ta.id, ta.action
                ),
            }
        }
        println!();
    }

    if !review.new_threads.is_empty() {
        println!("New Threads");
        for nt in &review.new_threads {
            let norm = nt.file.trim_start_matches('/');
            let exists = changed_files
                .iter()
                .any(|f| f.path.trim_start_matches('/') == norm);
            if exists {
                println!("  ✅ {} exists in repo", nt.file);
            } else {
                println!(
                    "  ⚠️  {} is outside the PR diff — will still be posted",
                    nt.file
                );
            }
        }
        println!();
    }

    if !review.open_points.is_empty() {
        println!("Open Points");
        for op in &review.open_points {
            let matched = open_points_ctx
                .iter()
                .any(|p| p.to_lowercase().contains(&op.ref_.to_lowercase()));
            if matched {
                println!("  ✅ \"{}\" matches context open point", op.ref_);
            } else {
                println!("  ❌ \"{}\" not found in context open points", op.ref_);
            }
        }
        println!();
    }

    let error_count = errors.len();
    let warning_count = warnings.len();

    if error_count > 0 || warning_count > 0 {
        if error_count > 0 {
            println!(
                "Result: {} error{}, {} warning{} — fix errors before applying.",
                error_count,
                if error_count == 1 { "" } else { "s" },
                warning_count,
                if warning_count == 1 { "" } else { "s" }
            );
            std::process::exit(1);
        } else {
            println!(
                "Result: {} warning{} — apply can proceed with --force",
                warning_count,
                if warning_count == 1 { "" } else { "s" }
            );
            std::process::exit(2);
        }
    } else {
        println!("Result: valid — no errors or warnings.");
    }

    Ok(())
}

pub async fn apply(
    file: String,
    pr_id: Option<String>,
    format: Option<String>,
    dry_run: bool,
    force: bool,
) -> Result<()> {
    let config = Config::load()?;
    let provider_set = ProviderSet::from_config(&config)?;
    let tracker = provider_set.issue_tracker;
    let vcs = provider_set.vcs;
    let git = LocalGitProvider;
    let repo_name = git.get_repo_name()?;

    let resolved_pr_id =
        super::resolve_pr_id(pr_id, vcs.as_ref(), tracker.as_ref(), &git, &repo_name).await?;

    let fmt = detect_format(&file, format.as_deref());
    let review = if fmt == "md" {
        parse_md(&file)?
    } else {
        parse_yaml(&file)?
    };

    let threads = vcs
        .get_pull_request_threads(&repo_name, &resolved_pr_id)
        .await
        .unwrap_or_default();
    let changed_files = vcs
        .get_pull_request_changed_files(&repo_name, &resolved_pr_id)
        .await
        .unwrap_or_default();
    let pr = vcs
        .get_pull_request_details(&repo_name, &resolved_pr_id)
        .await?;
    let open_points_ctx = super::extract_open_points(pr.description.as_deref().unwrap_or(""));

    let (errors, warnings) = validate_review(&review, &threads, &changed_files, &open_points_ctx);

    if !errors.is_empty() {
        for e in &errors {
            println!("❌ {}", e);
        }
        return Err(anyhow!(
            "Validation failed with {} error(s) — nothing written.",
            errors.len()
        ));
    }

    if !warnings.is_empty() && !force {
        for w in &warnings {
            println!("⚠️  {}", w);
        }
        return Err(anyhow!(
            "{} warning(s) — re-run with --force to apply anyway.",
            warnings.len()
        ));
    }

    println!("Applying review to PR #{}…\n", resolved_pr_id);

    let reply_actions: Vec<_> = review
        .threads
        .iter()
        .filter(|t| t.action == "reply")
        .collect();
    let resolve_actions: Vec<_> = review
        .threads
        .iter()
        .filter(|t| t.action == "resolve")
        .collect();

    let total = 1
        + reply_actions.len()
        + resolve_actions.len()
        + review.new_threads.len()
        + if review.open_points.is_empty() { 0 } else { 1 };
    let mut step = 0usize;
    let mut applied = 0usize;
    let mut failed = 0usize;

    // Step 1: post summary thread
    step += 1;
    let summary_thread_id: Option<String>;
    if dry_run {
        println!(
            "[{}/{}] Would post summary comment as new PR thread",
            step, total
        );
        summary_thread_id = None;
    } else {
        let summary_content = format!("## AI Review Summary\n\n{}", review.summary);
        match vcs
            .add_pull_request_thread(&repo_name, &resolved_pr_id, &summary_content, None, None)
            .await
        {
            Ok(t) => {
                println!(
                    "[{}/{}] Posting summary comment…              ✅ Thread {} created",
                    step, total, t.id
                );
                summary_thread_id = Some(t.id);
                applied += 1;
            }
            Err(e) => {
                println!(
                    "[{}/{}] Posting summary comment…              ❌ {}",
                    step, total, e
                );
                summary_thread_id = None;
                failed += 1;
            }
        }
    }

    // Step 2: replies
    for ta in &reply_actions {
        step += 1;
        let live = threads.iter().find(|t| t.id == ta.id.to_string());
        if live.map(|t| t.status.as_str()).unwrap_or("") != "active" {
            println!(
                "[{}/{}] Replying to thread {}…                ⚠️  skipped (not active)",
                step, total, ta.id
            );
            continue;
        }
        if dry_run {
            println!(
                "[{}/{}] Would reply to thread {} with: {}",
                step,
                total,
                ta.id,
                &ta.comment.chars().take(60).collect::<String>()
            );
        } else {
            match vcs
                .reply_to_pull_request_thread(
                    &repo_name,
                    &resolved_pr_id,
                    &ta.id.to_string(),
                    &ta.comment,
                )
                .await
            {
                Ok(_) => {
                    println!(
                        "[{}/{}] Replying to thread {}…                ✅ Reply posted",
                        step, total, ta.id
                    );
                    applied += 1;
                }
                Err(e) => {
                    println!(
                        "[{}/{}] Replying to thread {}…                ❌ {}",
                        step, total, ta.id, e
                    );
                    failed += 1;
                }
            }
        }
    }

    // Step 3: resolves
    for ta in &resolve_actions {
        step += 1;
        let live = threads.iter().find(|t| t.id == ta.id.to_string());
        if live.map(|t| t.status.as_str()).unwrap_or("") != "active" {
            println!(
                "[{}/{}] Resolving thread {}…                  ⚠️  skipped (not active)",
                step, total, ta.id
            );
            continue;
        }
        if dry_run {
            println!(
                "[{}/{}] Would resolve thread {} (status → fixed)",
                step, total, ta.id
            );
        } else {
            if !ta.comment.is_empty() {
                let _ = vcs
                    .reply_to_pull_request_thread(
                        &repo_name,
                        &resolved_pr_id,
                        &ta.id.to_string(),
                        &ta.comment,
                    )
                    .await;
            }
            match vcs
                .update_pull_request_thread_status(
                    &repo_name,
                    &resolved_pr_id,
                    &ta.id.to_string(),
                    "fixed",
                )
                .await
            {
                Ok(_) => {
                    println!(
                        "[{}/{}] Resolving thread {}…                  ✅ Resolved",
                        step, total, ta.id
                    );
                    applied += 1;
                }
                Err(e) => {
                    println!(
                        "[{}/{}] Resolving thread {}…                  ❌ {}",
                        step, total, ta.id, e
                    );
                    failed += 1;
                }
            }
        }
    }

    // Step 4: new threads
    for nt in &review.new_threads {
        step += 1;
        let label = format!("{}:{}", nt.file, nt.line);
        if dry_run {
            println!(
                "[{}/{}] Would post {} thread on {}",
                step, total, nt.severity, label
            );
        } else {
            let fp = nt.file.trim_start_matches('/');
            match vcs
                .add_pull_request_thread(
                    &repo_name,
                    &resolved_pr_id,
                    &nt.comment,
                    Some(fp),
                    Some(nt.line),
                )
                .await
            {
                Ok(t) => {
                    println!(
                        "[{}/{}] Posting new thread on {}…   ✅ Thread {} created",
                        step, total, label, t.id
                    );
                    applied += 1;
                }
                Err(e) => {
                    println!(
                        "[{}/{}] Posting new thread on {}…   ❌ {}",
                        step, total, label, e
                    );
                    failed += 1;
                }
            }
        }
    }

    // Step 5: open points summary as reply on summary thread
    if !review.open_points.is_empty() {
        step += 1;
        let mut op_text = String::from("**Open Points Status**\n\n");
        for op in &review.open_points {
            let icon = match op.status.as_str() {
                "addressed" => "✅",
                "partially_addressed" => "⚠️",
                _ => "❌",
            };
            op_text.push_str(&format!(
                "{} **{}** — {}: {}\n",
                icon, op.ref_, op.status, op.comment
            ));
        }

        if dry_run {
            println!(
                "[{}/{}] Would post open points summary on summary thread",
                step, total
            );
        } else if let Some(ref sid) = summary_thread_id {
            match vcs
                .reply_to_pull_request_thread(&repo_name, &resolved_pr_id, sid, &op_text)
                .await
            {
                Ok(_) => {
                    println!(
                        "[{}/{}]       Open points summary…                  ✅ Posted as reply on thread {}",
                        step, total, sid
                    );
                    applied += 1;
                }
                Err(e) => {
                    println!(
                        "[{}/{}]       Open points summary…                  ❌ {}",
                        step, total, e
                    );
                    failed += 1;
                }
            }
        }
    }

    println!();
    if dry_run {
        println!("Dry run complete. {} actions would be applied.", total);
    } else {
        println!("Done. {} actions applied, {} failed.", applied, failed);
        if failed > 0 {
            std::process::exit(2);
        }
    }

    Ok(())
}
