pub async fn hold(stash: bool, force: bool, stay: bool) -> anyhow::Result<()> {
    // SPECIFICATION:
    // 1. If baseline: exit.
    // 2. Check git status.
    // 3. If dirty and no flags: prompt for --stash or --force.
    // 4. If --stash: git stash push -m "stash-{wi-id}-{slug}".
    // 5. git push, then switch to baseline.
    //
    // PSEUDO-CODE:
    // 1. Validate activity context.
    // 2. If dirty:
    //    - If stash: git stash push.
    //    - If force: git checkout -- .
    //    - Else: error "working tree dirty".
    // 3. git push.
    // 4. git checkout baseline.
    println!("Scaffold: fm task hold --stash {} --force {} --stay {}", stash, force, stay);
    Ok(())
}

pub async fn update(
    _title: Option<String>,
    _state: Option<String>,
    _description: Option<String>,
    _assigned_to: Option<String>,
    _tags: Option<String>,
) -> anyhow::Result<()> {
    // SPECIFICATION:
    // Apply requested fields via ADO WI PATCH.
    //
    // PSEUDO-CODE:
    // 1. Parse WI ID from current branch.
    // 2. IssueTracker::update_work_item(id, fields).
    println!("Scaffold: fm task update");
    Ok(())
}

pub async fn complete() -> anyhow::Result<()> {
    // SPECIFICATION:
    // 1. Verify WI closed and PR merged/abandoned.
    // 2. Switch to baseline, git pull.
    //
    // PSEUDO-CODE:
    // 1. Check WI and PR state.
    // 2. If not finalized, error.
    // 3. git checkout baseline && git pull.
    println!("Scaffold: fm task complete");
    Ok(())
}

pub async fn sync(rebase: bool, check: bool) -> anyhow::Result<()> {
    // SPECIFICATION:
    // Update activity branch with commits from baseline.
    // - --check: dry-run, show ahead/behind.
    // - Default: git merge origin/{target}.
    // - --rebase: git rebase origin/{target}.
    // - Conflicts: exit and let user resolve.
    //
    // PSEUDO-CODE:
    // 1. git fetch origin.
    // 2. If check: print divergence and exit.
    // 3. Perform merge or rebase.
    // 4. If conflict, print instructions and exit non-zero.
    // 5. git push.
    println!("Scaffold: fm task sync --rebase {} --check {}", rebase, check);
    Ok(())
}
