use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(author, version = option_env!("FM_VERSION").unwrap_or(env!("CARGO_PKG_VERSION")), about, long_about = None)]
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
    /// Manage the current activity
    #[command(alias = "t")]
    Task {
        #[command(subcommand)]
        command: TaskCommands,
    },
    /// Manage pull requests
    #[command(alias = "p")]
    Pr {
        #[command(subcommand)]
        command: PrCommands,
    },
    /// Manage pipelines
    #[command(aliases = ["pl", "pipe"])]
    Pipeline {
        #[command(subcommand)]
        command: PipelineCommands,
    },
    /// Manage child tasks
    #[command(alias = "td")]
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
    #[command(alias = "ctx")]
    Context {
        /// Show only work item details
        #[arg(long)]
        only_task: bool,
        /// Show only PR details
        #[arg(long)]
        only_pr: bool,
        /// Show only git details
        #[arg(long)]
        only_git: bool,
        /// Show only pipeline details
        #[arg(long)]
        only_pipeline: bool,
        /// Show comments for the current work item
        #[arg(short, long)]
        task_comments: bool,
    },
    /// Commit changes, handling the docs submodule transparently
    // SPECIFICATION:
    // Commit staged changes, handling the _docs submodule when it has pending changes.
    // - If _docs has uncommitted changes: commit and push _docs first.
    // - If _docs has unpushed commits: push _docs first.
    // - Then commit the main repo, updating the submodule pointer if needed.
    // - Auto-generate message from WI if omitted.
    #[command(alias = "c")]
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
        #[arg(short, long)]
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
    #[command(alias = "pu")]
    Push {
        /// Use --force-with-lease
        #[arg(short, long)]
        force: bool,
        /// Skip docs submodule handling
        #[arg(short, long)]
        no_docs: bool,
    },
    /// Commit and push in one step
    // SPECIFICATION:
    // Shorthand for fm commit --all && fm push.
    #[command(alias = "s")]
    Sync {
        /// Commit message
        #[arg(short, long)]
        message: Option<String>,
        /// Docs submodule commit message
        #[arg(short, long)]
        docs_message: Option<String>,
    },
    /// Manage SonarQube issues and projects
    // SPECIFICATION:
    // List projects or show issues.
    #[command(alias = "sq")]
    Sonar {
        #[command(subcommand)]
        command: SonarCommands,
    },
    /// Run diagnostic checks
    #[command(alias = "dr")]
    Doctor {
        /// Attempt to fix broken invariants
        #[arg(short, long)]
        fix: bool,
    },
    /// Low-level plumbing commands
    #[command(subcommand, alias = "plumb")]
    Plumbing(PlumbingCommands),
    /// Initialize a new configuration file
    #[command(alias = "i")]
    Init {
        /// Output file path (default: fm.toml)
        #[arg(short, long)]
        path: Option<String>,
        /// Discover config from .env file and git remote
        #[arg(short, long)]
        discover: bool,
    },
    /// Show the current version
    Version,
}

#[derive(Subcommand)]
pub enum TaskCommands {
    /// Create a WI, branch, and draft PR, then switch locally
    // SPECIFICATION:
    // 1. Create ADO WI (User Story/Bug).
    // 2. If --sonar-project, append open issues to description.
    // 3. Derive branch name: {type}/{wi-id}-{slug}.
    // 4. Create remote branch from --target.
    // 5. Create draft PR linked to WI.
    // 6. Set WI state to Active.
    // 7. git fetch && git checkout branch.
    #[command(alias = "n")]
    New {
        /// Work item title
        #[arg(short, long)]
        title: String,
        /// Work item description
        #[arg(short, long)]
        description: Option<String>,
        /// Branch slug suffix
        #[arg(short, long)]
        branch: Option<String>,
        /// feature or fix
        #[arg(long, default_value = "feature")]
        type_name: String,
        /// Target baseline branch
        #[arg(long)]
        target: Option<String>,
        /// Assigned-to
        #[arg(short, long)]
        assigned_to: Option<String>,
        /// Semicolon-separated tags
        #[arg(long)]
        tags: Option<String>,
        /// SonarQube project key
        #[arg(short, long)]
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
    //    - Restore stash named stash-{wi-id}-staged / stash-{wi-id}-unstaged.
    #[command(alias = "l")]
    Load {
        /// WI id, PR id, or branch name
        id: String,
        /// Target baseline branch
        #[arg(short, long)]
        target: Option<String>,
        /// Initialize branch and PR if missing
        #[arg(short, long)]
        init: bool,
        /// Specify branch name for initialization
        #[arg(short, long)]
        branch: Option<String>,
    },
    /// List work items
    // SPECIFICATION:
    // Runs WIQL query scoped to the ADO project with the given filters.
    #[command(alias = "ls")]
    List {
        /// Filter by current user
        #[arg(short, long)]
        mine: bool,
        /// State filter
        #[arg(short, long, default_value = "Active")]
        state: String,
        /// feature, fix, all
        #[arg(short, long, default_value = "all")]
        type_name: String,
        /// Maximum results
        #[arg(short, long, default_value_t = 20)]
        max: i32,
    },
    /// Show work item details
    // SPECIFICATION:
    // Display detailed work item information, optionally with comments.
    // Without id: shows the current activity work item (error if not in Activity context).
    #[command(alias = "sh")]
    Show {
        /// WI id, PR id, or branch name (optional, uses current context if omitted)
        id: Option<String>,
        /// Hide comments (default: show comments)
        #[arg(short, long)]
        no_comments: bool,
        /// Show compact format (id, status, title, comments count, branch, PR)
        #[arg(short, long)]
        compact: bool,
    },
    /// Add a comment to the current work item
    // SPECIFICATION:
    // 1. Derive WI from current branch.
    // 2. Add comment via the issue tracker API.
    #[command(alias = "c")]
    Comment {
        /// Comment text
        #[arg(short, long)]
        message: String,
    },
    /// Pause the current activity
    // SPECIFICATION:
    // 1. If baseline: exit.
    // 2. Check git status.
    // 3. If dirty: auto-stash (staged to stash-{wi-id}-staged, unstaged to stash-{wi-id}-unstaged).
    //    Use --force to discard instead of stashing.
    // 4. git push, then switch to baseline.
    #[command(alias = "h")]
    Hold {
        /// Discard uncommitted changes instead of stashing
        #[arg(long)]
        force: bool,
        /// Stay on the current branch
        #[arg(long)]
        stay: bool,
    },
    /// Update the work item linked to the current activity
    // SPECIFICATION:
    // Apply requested fields via ADO WI PATCH.
    #[command(alias = "u")]
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
    #[command(alias = "done")]
    Complete,
    /// Sync the activity branch with the baseline
    // SPECIFICATION:
    // Update activity branch with commits from baseline.
    // - --check: dry-run, show ahead/behind.
    // - Default: git merge origin/{target}.
    // - --rebase: git rebase origin/{target}.
    // - Conflicts: exit and let user resolve.
    #[command(alias = "sy")]
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
    /// Assemble context.md for AI review
    #[command(alias = "sh")]
    Show {
        /// PR id, WI id, or branch
        id: Option<String>,
        /// Write output to file instead of stdout
        #[arg(long)]
        out: Option<String>,
        /// Inject README / AGENTS.md / CONTRIBUTING.md as project context
        #[arg(long)]
        include_project_context: bool,
    },
    /// Manage PR comment threads
    #[command(alias = "th")]
    Thread {
        #[command(subcommand)]
        command: PrThreadCommands,
    },
    /// Validate and apply AI review files
    #[command(alias = "fb")]
    Feedback {
        #[command(subcommand)]
        command: PrFeedbackCommands,
    },
    /// Add a comment to a PR
    // SPECIFICATION:
    // Add a comment to the PR.
    #[command(alias = "c")]
    Comment {
        /// PR id, WI id, or branch (optional, uses current context if omitted)
        id: Option<String>,
        /// Comment text
        #[arg(short, long)]
        message: String,
    },
    /// Update the PR linked to the current activity
    // SPECIFICATION:
    // Apply requested changes via ADO PR PATCH.
    // - --publish: isDraft=false.
    #[command(alias = "u")]
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
    #[command(alias = "m")]
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
    #[command(alias = "r")]
    Review {
        /// PR id, WI id, or branch
        id: String,
    },
}

#[derive(Subcommand)]
pub enum PrThreadCommands {
    /// List PR threads
    #[command(alias = "ls")]
    List {
        /// PR id, WI id, or branch (optional, uses current context if omitted)
        id: Option<String>,
        /// Filter by thread status: active | resolved | all
        #[arg(long, default_value = "active")]
        status: String,
    },
    /// Reply to a thread
    #[command(alias = "r")]
    Reply {
        /// Thread ID
        thread_id: String,
        /// Reply text, or "-" to read from stdin
        message: String,
        /// PR id (optional, uses current context if omitted)
        #[arg(long)]
        pr: Option<String>,
        /// Resolve thread after replying
        #[arg(long)]
        resolve: bool,
    },
    /// Resolve one or more threads
    #[command(alias = "res")]
    Resolve {
        /// One or more thread IDs
        thread_ids: Vec<String>,
        /// PR id (optional, uses current context if omitted)
        #[arg(long)]
        pr: Option<String>,
        /// Optional comment to post before resolving
        #[arg(long)]
        comment: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum PrFeedbackCommands {
    /// Validate a review file against the PR
    #[command(alias = "v")]
    Validate {
        /// Path to review.yaml or review.md
        #[arg(long)]
        file: String,
        /// PR id (optional, uses current context if omitted)
        #[arg(long)]
        pr: Option<String>,
        /// Explicit format: yaml | md
        #[arg(long)]
        format: Option<String>,
    },
    /// Apply a review file to the PR
    #[command(alias = "a")]
    Apply {
        /// Path to review.yaml or review.md
        #[arg(long)]
        file: String,
        /// PR id (optional, uses current context if omitted)
        #[arg(long)]
        pr: Option<String>,
        /// Explicit format: yaml | md
        #[arg(long)]
        format: Option<String>,
        /// Print API calls without writing
        #[arg(long)]
        dry_run: bool,
        /// Apply despite warnings
        #[arg(long)]
        force: bool,
    },
    /// Describe the review file format in plain text
    #[command(alias = "st")]
    Structure,
    /// Print the JSON schema for review.yaml
    #[command(alias = "sc")]
    Schema,
}

#[derive(Subcommand)]
pub enum PipelineCommands {
    /// Run a pipeline for the current branch
    // SPECIFICATION:
    // Trigger a CI pipeline for the current branch.
    #[command(alias = "r")]
    Run {
        /// Pipeline definition ID
        #[arg(long)]
        id: Option<String>,
    },
    /// Show pipeline status for the current branch
    // SPECIFICATION:
    // Show the latest CI run status for the current branch.
    // - --watch: poll until completed.
    #[command(alias = "st")]
    Status {
        /// Run ID
        #[arg(long)]
        run_id: Option<String>,
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
    #[command(alias = "sh")]
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
    #[command(alias = "n")]
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
    #[command(alias = "p")]
    Pick {
        /// Task id or title fragment
        reference: String,
    },
    /// Set a todo Closed
    // SPECIFICATION:
    // Set a todo to Closed.
    #[command(alias = "done")]
    Complete {
        /// Task id or title fragment
        reference: String,
    },
    /// Set a todo back to New
    // SPECIFICATION:
    // Set a todo back to New.
    #[command(alias = "ro")]
    Reopen {
        /// Task id or title fragment
        reference: String,
    },
    /// Update a todo
    // SPECIFICATION:
    // Update a todo's title, description, or assignment.
    #[command(alias = "u")]
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
    #[command(alias = "nx")]
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
    WiGet { id: String },
}

#[derive(Subcommand)]
pub enum SonarCommands {
    /// List projects
    #[command(alias = "ls")]
    List {
        /// Wildcard search on project name/key
        #[arg(short, long)]
        search: Option<String>,
        /// Only favorited projects
        #[arg(short, long)]
        favorites: bool,
    },
    /// Show SonarQube issues
    #[command(alias = "iss")]
    Issues {
        /// SonarQube project key (uses default from config if not provided)
        #[arg(short, long)]
        project: Option<String>,
        /// Fetch issues for all configured projects
        #[arg(short, long)]
        all: bool,
        /// Comma-separated severities
        #[arg(short, long)]
        severity: Option<String>,
        /// Maximum issues per project
        #[arg(short, long, default_value_t = 20)]
        max: i32,
    },
}

pub fn parse() -> Cli {
    Cli::parse()
}
