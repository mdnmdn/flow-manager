pub async fn show(all: bool, detail: bool) -> anyhow::Result<()> {
    // SPECIFICATION:
    // List all child Tasks (todos) of the current User Story.
    //
    // PSEUDO-CODE:
    // 1. Get current WI ID.
    // 2. IssueTracker::get_child_work_items(id, type="Task").
    // 3. Format and display.
    println!("Scaffold: fm todo show --all {} --detail {}", all, detail);
    Ok(())
}

pub async fn new(
    title: String,
    _description: Option<String>,
    _assigned_to: Option<String>,
    _pick: bool,
) -> anyhow::Result<()> {
    // SPECIFICATION:
    // Add a new child Task under the current User Story.
    //
    // PSEUDO-CODE:
    // 1. Get current WI ID.
    // 2. IssueTracker::create_work_item(title, type="Task", ...).
    // 3. IssueTracker::link_work_items(parent_id, child_id, "Child").
    // 4. If pick: IssueTracker::update_work_item_state(child_id, "Active").
    println!("Scaffold: fm todo new --title {}", title);
    Ok(())
}

pub async fn pick(reference: String) -> anyhow::Result<()> {
    // SPECIFICATION:
    // Set a todo to Active.
    //
    // PSEUDO-CODE:
    // 1. Resolve reference to Task ID.
    // 2. IssueTracker::update_work_item_state(id, "Active").
    println!("Scaffold: fm todo pick {}", reference);
    Ok(())
}

pub async fn complete(reference: String) -> anyhow::Result<()> {
    // SPECIFICATION:
    // Set a todo to Closed.
    //
    // PSEUDO-CODE:
    // 1. Resolve reference to Task ID.
    // 2. IssueTracker::update_work_item_state(id, "Closed").
    println!("Scaffold: fm todo complete {}", reference);
    Ok(())
}

pub async fn reopen(reference: String) -> anyhow::Result<()> {
    // SPECIFICATION:
    // Set a todo back to New.
    //
    // PSEUDO-CODE:
    // 1. Resolve reference to Task ID.
    // 2. IssueTracker::update_work_item_state(id, "New").
    println!("Scaffold: fm todo reopen {}", reference);
    Ok(())
}

pub async fn update(
    reference: String,
    _title: Option<String>,
    _description: Option<String>,
    _assigned_to: Option<String>,
    _state: Option<String>,
) -> anyhow::Result<()> {
    // SPECIFICATION:
    // Update a todo's title, description, or assignment.
    //
    // PSEUDO-CODE:
    // 1. Resolve reference to Task ID.
    // 2. IssueTracker::update_work_item(id, fields).
    println!("Scaffold: fm todo update {}", reference);
    Ok(())
}

pub async fn next(pick: bool) -> anyhow::Result<()> {
    // SPECIFICATION:
    // Show the next open todo (creation order).
    //
    // PSEUDO-CODE:
    // 1. Fetch child todos with state "New".
    // 2. Pick the first one.
    // 3. If pick: IssueTracker::update_work_item_state(id, "Active").
    println!("Scaffold: fm todo next --pick {}", pick);
    Ok(())
}
