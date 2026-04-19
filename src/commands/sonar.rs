use crate::cli::SonarCommands;
use crate::core::config::SonarConfig;
use crate::providers::sonar::SonarProvider;
use crate::providers::QualityProvider;
use anyhow::Result;
use tokio::task;

pub async fn run(command: SonarCommands, config: &SonarConfig) -> Result<()> {
    let sonar = SonarProvider::new(config)?;

    match command {
        SonarCommands::List { search, favorites } => {
            run_list(sonar, search, favorites).await?;
        }
        SonarCommands::Issues {
            project,
            all,
            severity,
            max,
        } => {
            if all {
                if config.projects.is_empty() {
                    return Err(anyhow::anyhow!(
                        "No projects configured in [sonar].projects"
                    ));
                }
                run_all(config, &config.projects, severity.as_deref(), max).await?;
            } else {
                let project_key = project
                    .or_else(|| config.projects.first().cloned())
                    .ok_or_else(|| {
                        anyhow::anyhow!("Project key is required (or configure [sonar].projects)")
                    })?;
                run_issues(&sonar, &project_key, severity.as_deref(), max).await?;
            }
        }
    }

    Ok(())
}

async fn run_list(sonar: SonarProvider, search: Option<String>, favorites: bool) -> Result<()> {
    let projects = sonar.list_projects(search.as_deref(), favorites).await?;

    if projects.is_empty() {
        println!("No projects found.");
        return Ok(());
    }

    println!("## SonarQube Projects");
    println!();

    for project in projects {
        let date = project
            .last_analysis
            .as_ref()
            .and_then(|d| d.get(0..10))
            .unwrap_or("-");

        if let Some(search) = &search {
            if !search.contains('*') && !search.contains('?') {
                println!("{} — {} ({})", project.key, project.name, date);
                continue;
            }
        }

        println!("{} — {}", project.key, project.name);
    }

    Ok(())
}

async fn run_issues(
    sonar: &SonarProvider,
    project_key: &str,
    severity: Option<&str>,
    max: i32,
) -> Result<()> {
    let issues = sonar.get_open_issues(project_key, severity).await?;

    if issues.is_empty() {
        println!("No open issues found for `{}`.", project_key);
        return Ok(());
    }

    println!("## Sonar Issues — `{}`", project_key);
    println!();

    for (i, issue) in issues.into_iter().enumerate().take(max as usize) {
        if i > 0 {
            println!();
        }
        let location = match (
            issue.start_line,
            issue.end_line,
            issue.start_offset,
            issue.end_offset,
        ) {
            (Some(start), Some(end), Some(start_off), Some(end_off)) => {
                if start == end {
                    Some(format!("L{}[{}-{}]", start, start_off, end_off))
                } else {
                    Some(format!("L{}-{}[{}-{}]", start, end, start_off, end_off))
                }
            }
            (Some(start), _, _, _) => Some(format!("L{}", start)),
            _ => None,
        };

        let location_suffix = location.map(|l| format!(" {}", l)).unwrap_or_default();

        println!(
            "### {}. {}{}\n{}\n{}{}",
            i + 1,
            issue.severity,
            issue.key,
            issue.message,
            issue.component,
            location_suffix
        );
    }

    Ok(())
}

async fn run_all(
    config: &SonarConfig,
    projects: &[String],
    severity: Option<&str>,
    max: i32,
) -> Result<()> {
    let mut handles = Vec::new();

    for p in projects {
        let config = config.clone();
        let severity = severity.map(String::from);
        let project_key = p.clone();
        handles.push(task::spawn(async move {
            let sonar = SonarProvider::new(&config)?;
            run_issues(&sonar, &project_key, severity.as_deref(), max).await
        }));
    }

    for handle in handles {
        handle.await??;
        println!();
    }

    Ok(())
}
