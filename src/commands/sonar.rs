pub async fn run(project: Option<String>, _severity: Option<String>, _max: i32) -> anyhow::Result<()> {
    // SPECIFICATION:
    // Show SonarQube issues relevant to the current context.
    //
    // PSEUDO-CODE:
    // 1. Identify project key from tags or config.
    // 2. Fetch issues from SonarQube API filtered by severity and project.
    // 3. Format and display as list.
    println!("Scaffold: fm sonar --project {:?}", project);
    Ok(())
}
