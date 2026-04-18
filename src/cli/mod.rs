use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Output format
    #[arg(short, long, default_value = "markdown")]
    pub format: String,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Manage work activities
    Work {
        #[command(subcommand)]
        command: WorkCommands,
    },
    /// Manage the current activity
    Task {
        #[command(subcommand)]
        command: TaskCommands,
    },
    /// Manage pull requests
    Pr {
        #[command(subcommand)]
        command: PrCommands,
    },
    /// Manage pipelines
    Pipeline {
        #[command(subcommand)]
        command: PipelineCommands,
    },
    /// Manage child tasks
    Todo {
        #[command(subcommand)]
        command: TodoCommands,
    },
    /// Show the current workflow context
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
    Context {
        /// Show only work item details
        #[arg(long)]
        only_wi: bool,
        /// Show only PR details
        #[arg(long)]
        only_pr: bool,
        /// Show only git details
        #[arg(long)]
        only_git: bool,
        /// Show only pipeline details
        #[arg(long)]
        only_pipeline: bool,
    },
    /// Commit changes, handling the docs submodule transparently
    // SPECIFICATION:
    // Commit staged changes, handling the _docs submodule when it has pending changes.
    // - If _docs has uncommitted changes: commit and push _docs first.
    // - If _docs has unpushed commits: push _docs first.
    // - Then commit the main repo, updating the submodule pointer if needed.
    // - Auto-generate message from WI if omitted.
    //
    // PSEUDO-CODE:
    // 1. If in activity branch and message is None, fetch WI title for auto-message.
    // 2. Check _docs submodule status.
    // 3. If _docs dirty:
    //    - git add -A in _docs.
    //    - git commit in _docs with docs_message.
    //    - git push in _docs.
    //    - git add _docs in main repo.
    // 4. Perform git commit in main repo.
    Commit {
        /// Commit message
        #[arg(short, long)]
        message: Option<String>,
        /// Stage tracked changes before commit
        #[arg(short, long)]
        all: bool,
        /// Amend the previous commit
        #[arg(long)]
        amend: bool,
        /// Override docs submodule commit message
        #[arg(long)]
        docs_message: Option<String>,
        /// Skip docs submodule handling
        #[arg(long)]
        no_docs: bool,
    },
    /// Push the current branch, including docs handling
    // SPECIFICATION:
    // Push current branch to remote, with submodule-awareness.
    // - Checks _docs for unpushed commits.
    // - Push _docs first if needed, then stage and commit pointer.
    //
    // PSEUDO-CODE:
    // 1. Check _docs status for unpushed commits.
    // 2. If _docs ahead:
    //    - Push _docs.
    //    - Commit pointer update in main repo if needed.
    // 3. git push main repo branch (with --force-with-lease if requested).
    Push {
        /// Use --force-with-lease
        #[arg(long)]
        force: bool,
        /// Skip docs submodule handling
        #[arg(long)]
        no_docs: bool,
    },
    /// Commit and push in one step
    // SPECIFICATION:
    // Shorthand for fm commit --all && fm push.
    //
    // PSEUDO-CODE:
    // 1. Execute Commit command with all=true.
    // 2. Execute Push command.
    Sync {
        /// Commit message
        #[arg(short, long)]
        message: Option<String>,
        /// Docs submodule commit message
        #[arg(long)]
        docs_message: Option<String>,
    },
    /// Show SonarQube issues
    // SPECIFICATION:
    // Show SonarQube issues relevant to the current context.
    //
    // PSEUDO-CODE:
    // 1. Identify project key from tags or config.
    // 2. Fetch issues from SonarQube API filtered by severity and project.
    // 3. Format and display as list.
    Sonar {
        /// SonarQube project key
        #[arg(short, long)]
        project: Option<String>,
        /// Comma-separated severities
        #[arg(short, long)]
        severity: Option<String>,
        /// Maximum issues
        #[arg(short, long, default_value_t = 20)]
        max: i32,
    },
    /// Low-level plumbing commands
    #[command(subcommand)]
    Plumbing(PlumbingCommands),
}

#[derive(Subcommand)]
pub enum WorkCommands {
    /// Create a WI, branch, and draft PR, then switch locally
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
    New {
        /// Work item title
        #[arg(long)]
        title: String,
        /// Work item description
        #[arg(long)]
        description: Option<String>,
        /// Branch slug suffix
        #[arg(long)]
        branch: Option<String>,
        /// feature or fix
        #[arg(long, default_value = "feature")]
        type_name: String,
        /// Target baseline branch
        #[arg(long)]
        target: Option<String>,
        /// Assigned-to
        #[arg(long)]
        assigned_to: Option<String>,
        /// Semicolon-separated tags
        #[arg(long)]
        tags: Option<String>,
        /// SonarQube project key
        #[arg(long)]
        sonar_project: Option<String>,
    },
    /// Resume an existing work item
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
    Load {
        /// WI id, PR id, or branch name
        id: String,
        /// Target baseline branch
        #[arg(long)]
        target: Option<String>,
    },
    /// List work items
    // SPECIFICATION:
    // Runs WIQL query scoped to the ADO project with the given filters.
    //
    // PSEUDO-CODE:
    // 1. Build WIQL query based on --mine, --state, --type_name.
    // 2. Call IssueTracker::query_work_items(wiql).
    // 3. Format results as table.
    List {
        /// Filter by current user
        #[arg(long)]
        mine: bool,
        /// State filter
        #[arg(long, default_value = "Active")]
        state: String,
        /// feature, fix, all
        #[arg(long, default_value = "all")]
        type_name: String,
        /// Maximum results
        #[arg(long, default_value_t = 20)]
        max: i32,
    },
}

#[derive(Subcommand)]
pub enum TaskCommands {
    /// Pause the current activity
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
    Hold {
        /// Stash uncommitted changes
        #[arg(long)]
        stash: bool,
        /// Discard uncommitted changes
        #[arg(long)]
        force: bool,
        /// Stay on the current branch
        #[arg(long)]
        stay: bool,
    },
    /// Update the work item linked to the current activity
    // SPECIFICATION:
    // Apply requested fields via ADO WI PATCH.
    //
    // PSEUDO-CODE:
    // 1. Parse WI ID from current branch.
    // 2. IssueTracker::update_work_item(id, fields).
    Update {
        /// New title
        #[arg(long)]
        title: Option<String>,
        /// New state
        #[arg(long)]
        state: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
        /// New assigned-to
        #[arg(long)]
        assigned_to: Option<String>,
        /// New tags
        #[arg(long)]
        tags: Option<String>,
    },
    /// Return to baseline after the activity is done
    // SPECIFICATION:
    // 1. Verify WI closed and PR merged/abandoned.
    // 2. Switch to baseline, git pull.
    //
    // PSEUDO-CODE:
    // 1. Check WI and PR state.
    // 2. If not finalized, error.
    // 3. git checkout baseline && git pull.
    Complete,
    /// Sync the activity branch with the baseline
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
    Sync {
        /// Use rebase instead of merge
        #[arg(long)]
        rebase: bool,
        /// Dry-run only
        #[arg(long)]
        check: bool,
    },
}

#[derive(Subcommand)]
pub enum PrCommands {
    /// Show PR details
    // SPECIFICATION:
    // Fetch PR details: title, state, draft, reviewers, merge status, linked WIs.
    //
    // PSEUDO-CODE:
    // 1. Resolve ID to PR.
    // 2. Call VCSProvider::get_pull_request_details(id).
    // 3. Render details.
    Show {
        /// PR id, WI id, or branch
        id: Option<String>,
    },
    /// Update the PR linked to the current activity
    // SPECIFICATION:
    // Apply requested changes via ADO PR PATCH.
    // - --publish: isDraft=false.
    //
    // PSEUDO-CODE:
    // 1. Resolve PR from current branch.
    // 2. VCSProvider::update_pull_request(id, fields).
    Update {
        /// New title
        #[arg(long)]
        title: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
        /// Publish the draft PR
        #[arg(long)]
        publish: bool,
        /// active, abandoned, completed
        #[arg(long)]
        status: Option<String>,
        /// Reviewer email/unique name
        #[arg(long)]
        add_reviewer: Vec<String>,
    },
    /// Complete the PR linked to the current activity
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
    Merge {
        /// Merge strategy
        #[arg(long)]
        strategy: Option<String>,
        /// Delete source branch after merge
        #[arg(long)]
        delete_source_branch: bool,
        /// Bypass branch policies
        #[arg(long)]
        bypass_policy: bool,
    },
    /// Switch to another PR branch for review
    // SPECIFICATION:
    // 1. Auto-stash current activity if dirty.
    // 2. Resolve ID to PR and its branch.
    // 3. git fetch && git checkout pr-branch.
    //
    // PSEUDO-CODE:
    // 1. Task::Hold logic (with --stash).
    // 2. Resolve target PR branch.
    // 3. git checkout target-branch.
    Review {
        /// PR id, WI id, or branch
        id: String,
    },
}

#[derive(Subcommand)]
pub enum PipelineCommands {
    /// Run a pipeline for the current branch
    // SPECIFICATION:
    // Trigger a CI pipeline for the current branch.
    //
    // PSEUDO-CODE:
    // 1. Detect branch.
    // 2. If id is None, list pipelines and exit.
    // 3. PipelineProvider::run_pipeline(id, branch).
    Run {
        /// Pipeline definition ID
        #[arg(long)]
        id: Option<i32>,
    },
    /// Show pipeline status for the current branch
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
    Status {
        /// Run ID
        #[arg(long)]
        run_id: Option<i32>,
        /// Poll until completed
        #[arg(long)]
        watch: bool,
    },
}

#[derive(Subcommand)]
pub enum TodoCommands {
    /// Show todos
    // SPECIFICATION:
    // List all child Tasks (todos) of the current User Story.
    //
    // PSEUDO-CODE:
    // 1. Get current WI ID.
    // 2. IssueTracker::get_child_work_items(id, type="Task").
    // 3. Format and display.
    Show {
        /// Include closed items
        #[arg(long)]
        all: bool,
        /// Include descriptions
        #[arg(long)]
        detail: bool,
    },
    /// Create a todo
    // SPECIFICATION:
    // Add a new child Task under the current User Story.
    //
    // PSEUDO-CODE:
    // 1. Get current WI ID.
    // 2. IssueTracker::create_work_item(title, type="Task", ...).
    // 3. IssueTracker::link_work_items(parent_id, child_id, "Child").
    // 4. If pick: IssueTracker::update_work_item_state(child_id, "Active").
    New {
        /// Todo title
        #[arg(long)]
        title: String,
        /// Description
        #[arg(long)]
        description: Option<String>,
        /// Assigned-to
        #[arg(long)]
        assigned_to: Option<String>,
        /// Set Active immediately
        #[arg(long)]
        pick: bool,
    },
    /// Set a todo Active
    // SPECIFICATION:
    // Set a todo to Active.
    //
    // PSEUDO-CODE:
    // 1. Resolve reference to Task ID.
    // 2. IssueTracker::update_work_item_state(id, "Active").
    Pick {
        /// Task id or title fragment
        reference: String,
    },
    /// Set a todo Closed
    // SPECIFICATION:
    // Set a todo to Closed.
    //
    // PSEUDO-CODE:
    // 1. Resolve reference to Task ID.
    // 2. IssueTracker::update_work_item_state(id, "Closed").
    Complete {
        /// Task id or title fragment
        reference: String,
    },
    /// Set a todo back to New
    // SPECIFICATION:
    // Set a todo back to New.
    //
    // PSEUDO-CODE:
    // 1. Resolve reference to Task ID.
    // 2. IssueTracker::update_work_item_state(id, "New").
    Reopen {
        /// Task id or title fragment
        reference: String,
    },
    /// Update a todo
    // SPECIFICATION:
    // Update a todo's title, description, or assignment.
    //
    // PSEUDO-CODE:
    // 1. Resolve reference to Task ID.
    // 2. IssueTracker::update_work_item(id, fields).
    Update {
        /// Task id or title fragment
        reference: String,
        /// New title
        #[arg(long)]
        title: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
        /// New assigned-to
        #[arg(long)]
        assigned_to: Option<String>,
        /// New state
        #[arg(long)]
        state: Option<String>,
    },
    /// Show the next New todo
    // SPECIFICATION:
    // Show the next open todo (creation order).
    //
    // PSEUDO-CODE:
    // 1. Fetch child todos with state "New".
    // 2. Pick the first one.
    // 3. If pick: IssueTracker::update_work_item_state(id, "Active").
    Next {
        /// Set Active immediately
        #[arg(long)]
        pick: bool,
    },
}

#[derive(Subcommand)]
pub enum PlumbingCommands {
    /// Git plumbing commands
    Git {
        #[command(subcommand)]
        command: GitPlumbingCommands,
    },
    /// Azure DevOps plumbing commands
    Ado {
        #[command(subcommand)]
        command: AdoPlumbingCommands,
    },
}

#[derive(Subcommand)]
pub enum GitPlumbingCommands {
    /// Get current branch
    BranchCurrent,
}

#[derive(Subcommand)]
pub enum AdoPlumbingCommands {
    /// Get work item
    WiGet { id: i32 },
}

pub fn parse() -> Cli {
    Cli::parse()
}
