pub async fn run(
    title: String,
    _description: Option<String>,
    _branch: Option<String>,
    _type_name: String,
    _target: Option<String>,
    _assigned_to: Option<String>,
    _tags: Option<String>,
    _sonar_project: Option<String>,
) -> anyhow::Result<()> {
    // SPECIFICATION:
    // 1. Create ADO WI (User Story/Bug).
    // 2. If --sonar-project, append open issues to description.
    // 3. Derive branch name: {type}/{wi-id}-{slug}.
    // 4. Create remote branch from --target.
    // 5. Create draft PR linked to WI.
    // 6. Set WI state to Active.
    // 7. git fetch && git checkout branch.
    //
    // PSEUDO-CODE:
    // 1. IssueTracker::create_work_item(title, type, ...).
    // 2. If sonar_project: fetch sonar issues and update WI description.
    // 3. VCSProvider::create_branch(new_branch_name, target).
    // 4. VCSProvider::create_pull_request(new_branch_name, target, is_draft=true).
    // 5. VCSProvider::create_artifact_link(wi_id, branch_url).
    // 6. VCSProvider::create_artifact_link(wi_id, pr_url).
    // 7. IssueTracker::update_work_item_state(wi_id, "Active").
    // 8. Local shell: git fetch && git checkout branch.
    println!("Scaffold: fm work new --title {}", title);
    Ok(())
}

pub async fn load(id: String, _target: Option<String>) -> anyhow::Result<()> {
    // SPECIFICATION:
    // 1. Resolve ID to WI.
    // 2. If Closed/Done: print summary and exit.
    // 3. If Active/New:
    //    - Derive branch name.
    //    - Create remote branch/PR if missing (idempotency).
    //    - Set WI Active.
    //    - git fetch && git checkout branch.
    //    - Restore stash named stash-{wi-id}-*.
    //
    // PSEUDO-CODE:
    // 1. Resolve ID (WI/PR/Branch).
    // 2. Fetch WI. If closed, return.
    // 3. Repair context: ensure branch exists, ensure draft PR exists, ensure links on WI.
    // 4. Local: git fetch && git checkout branch.
    // 5. Check for local stash and git stash pop if found.
    println!("Scaffold: fm work load {}", id);
    Ok(())
}

pub async fn list(mine: bool, state: String, type_name: String, _max: i32) -> anyhow::Result<()> {
    // SPECIFICATION:
    // Runs WIQL query scoped to the ADO project with the given filters.
    //
    // PSEUDO-CODE:
    // 1. Build WIQL query based on --mine, --state, --type_name.
    // 2. Call IssueTracker::query_work_items(wiql).
    // 3. Format results as table.
    println!("Scaffold: fm work list --mine {} --state {} --type {}", mine, state, type_name);
    Ok(())
}
