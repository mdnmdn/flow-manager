pub async fn run(id: Option<i32>) -> anyhow::Result<()> {
    // SPECIFICATION:
    // Trigger a CI pipeline for the current branch.
    //
    // PSEUDO-CODE:
    // 1. Detect branch.
    // 2. If id is None, list pipelines and exit.
    // 3. PipelineProvider::run_pipeline(id, branch).
    println!("Scaffold: fm pipeline run --id {:?}", id);
    Ok(())
}

pub async fn status(run_id: Option<i32>, watch: bool) -> anyhow::Result<()> {
    // SPECIFICATION:
    // Show the latest CI run status for the current branch.
    // - --watch: poll until completed.
    //
    // PSEUDO-CODE:
    // 1. Resolve run_id or get latest for branch.
    // 2. Loop if watch:
    //    - Fetch status.
    //    - Render.
    //    - Break if completed.
    println!(
        "Scaffold: fm pipeline status --run-id {:?} --watch {}",
        run_id, watch
    );
    Ok(())
}
