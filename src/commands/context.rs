pub async fn run(
    _only_wi: bool,
    _only_pr: bool,
    _only_git: bool,
    _only_pipeline: bool,
) -> anyhow::Result<()> {
    // SPECIFICATION:
    // Snapshot of the current activity. Entry point for every new work session.
    // - Baseline branch: print branch name and last 5 commits.
    // - Activity branch: extract WI id from branch name, then:
    //     a. Fetch WI details (id, title, state, assigned to, tags).
    //     b. Find PR for this branch (state, draft, mergeable, reviewer count).
    //     c. Run git status and git log ahead/behind.
    //     d. Fetch latest CI pipeline run for this branch.
    //
    // PSEUDO-CODE:
    // 1. Detect current branch via VCS provider.
    // 2. If branch is baseline (main/develop):
    //    - List last 5 commits.
    //    - Render baseline template.
    // 3. If branch is activity (feature/*, fix/*):
    //    - Parse WI ID from branch name.
    //    - Call IssueTracker::get_work_item(id).
    //    - Call VCSProvider::get_pull_request(branch).
    //    - Call VCSProvider::get_status() (ahead/behind/dirty).
    //    - Call PipelineProvider::get_latest_run(branch).
    //    - Render activity template with all gathered info.
    println!("Scaffold: fm context");
    Ok(())
}
