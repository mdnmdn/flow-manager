use crate::core::config::Config;
use crate::providers::sonar::SonarProvider;
use crate::providers::QualityProvider;
use anyhow::Result;

pub async fn run(project: Option<String>, severity: Option<String>, max: i32) -> Result<()> {
    let config = Config::load()?;
    let sonar_config = config
        .sonar
        .ok_or_else(|| anyhow::anyhow!("SonarQube not configured"))?;
    let sonar = SonarProvider::new(&sonar_config)?;

    let project_key = project.ok_or_else(|| anyhow::anyhow!("Project key is required"))?;
    let issues = sonar
        .get_open_issues(&project_key, severity.as_deref())
        .await?;

    println!("## Sonar Issues for `{}`", project_key);
    println!("| Severity | Message | Component |");
    println!("|---|---|---|");
    for issue in issues.into_iter().take(max as usize) {
        println!(
            "| {} | {} | {} |",
            issue.severity, issue.message, issue.component
        );
    }

    Ok(())
}
