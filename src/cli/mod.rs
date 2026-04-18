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
    Complete,
    /// Sync the activity branch with the baseline
    // SPECIFICATION:
    // Update activity branch with commits from baseline.
    // - --check: dry-run, show ahead/behind.
    // - Default: git merge origin/{target}.
    // - --rebase: git rebase origin/{target}.
    // - Conflicts: exit and let user resolve.
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
    Show {
        /// PR id, WI id, or branch
        id: Option<String>,
    },
    /// Update the PR linked to the current activity
    // SPECIFICATION:
    // Apply requested changes via ADO PR PATCH.
    // - --publish: isDraft=false.
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
    Run {
        /// Pipeline definition ID
        #[arg(long)]
        id: Option<i32>,
    },
    /// Show pipeline status for the current branch
    // SPECIFICATION:
    // Show the latest CI run status for the current branch.
    // - --watch: poll until completed.
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
    Pick {
        /// Task id or title fragment
        reference: String,
    },
    /// Set a todo Closed
    // SPECIFICATION:
    // Set a todo to Closed.
    Complete {
        /// Task id or title fragment
        reference: String,
    },
    /// Set a todo back to New
    // SPECIFICATION:
    // Set a todo back to New.
    Reopen {
        /// Task id or title fragment
        reference: String,
    },
    /// Update a todo
    // SPECIFICATION:
    // Update a todo's title, description, or assignment.
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
