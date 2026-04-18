pub async fn show(id: Option<String>) -> anyhow::Result<()> {
    // SPECIFICATION:
    // Fetch PR details: title, state, draft, reviewers, merge status, linked WIs.
    //
    // PSEUDO-CODE:
    // 1. Resolve ID to PR.
    // 2. Call VCSProvider::get_pull_request_details(id).
    // 3. Render details.
    println!("Scaffold: fm pr show {:?}", id);
    Ok(())
}

pub async fn update(
    _title: Option<String>,
    _description: Option<String>,
    publish: bool,
    _status: Option<String>,
    _add_reviewer: Vec<String>,
) -> anyhow::Result<()> {
    // SPECIFICATION:
    // Apply requested changes via ADO PR PATCH.
    // - --publish: isDraft=false.
    //
    // PSEUDO-CODE:
    // 1. Resolve PR from current branch.
    // 2. VCSProvider::update_pull_request(id, fields).
    println!("Scaffold: fm pr update --publish {}", publish);
    Ok(())
}

pub async fn merge(
    strategy: Option<String>,
    _delete_source_branch: bool,
    _bypass_policy: bool,
) -> anyhow::Result<()> {
    // SPECIFICATION:
    // Complete (merge) the PR using configured strategy.
    // - Error if draft.
    // - Error if not mergeable.
    // - Complete PR and Close WI.
    //
    // PSEUDO-CODE:
    // 1. Resolve PR from branch.
    // 2. Validate PR is not draft and is mergeable.
    // 3. VCSProvider::complete_pull_request(id, strategy, delete_source_branch).
    // 4. IssueTracker::update_work_item_state(wi_id, "Closed").
    println!("Scaffold: fm pr merge --strategy {:?}", strategy);
    Ok(())
}

pub async fn review(id: String) -> anyhow::Result<()> {
    // SPECIFICATION:
    // 1. Auto-stash current activity if dirty.
    // 2. Resolve ID to PR and its branch.
    // 3. git fetch && git checkout pr-branch.
    //
    // PSEUDO-CODE:
    // 1. Task::Hold logic (with --stash).
    // 2. Resolve target PR branch.
    // 3. git checkout target-branch.
    println!("Scaffold: fm pr review {}", id);
    Ok(())
}
