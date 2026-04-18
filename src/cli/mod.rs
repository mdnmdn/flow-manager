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
    Push {
        /// Use --force-with-lease
        #[arg(long)]
        force: bool,
        /// Skip docs submodule handling
        #[arg(long)]
        no_docs: bool,
    },
    /// Commit and push in one step
    Sync {
        /// Commit message
        #[arg(short, long)]
        message: Option<String>,
        /// Docs submodule commit message
        #[arg(long)]
        docs_message: Option<String>,
    },
    /// Show SonarQube issues
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
    Load {
        /// WI id, PR id, or branch name
        id: String,
        /// Target baseline branch
        #[arg(long)]
        target: Option<String>,
    },
    /// List work items
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
    Complete,
    /// Sync the activity branch with the baseline
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
    Show {
        /// PR id, WI id, or branch
        id: Option<String>,
    },
    /// Update the PR linked to the current activity
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
    Review {
        /// PR id, WI id, or branch
        id: String,
    },
}

#[derive(Subcommand)]
pub enum PipelineCommands {
    /// Run a pipeline for the current branch
    Run {
        /// Pipeline definition ID
        #[arg(long)]
        id: Option<i32>,
    },
    /// Show pipeline status for the current branch
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
    Show {
        /// Include closed items
        #[arg(long)]
        all: bool,
        /// Include descriptions
        #[arg(long)]
        detail: bool,
    },
    /// Create a todo
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
    Pick {
        /// Task id or title fragment
        reference: String,
    },
    /// Set a todo Closed
    Complete {
        /// Task id or title fragment
        reference: String,
    },
    /// Set a todo back to New
    Reopen {
        /// Task id or title fragment
        reference: String,
    },
    /// Update a todo
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
