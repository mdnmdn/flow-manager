use app::cli;
use app::commands;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let cli = cli::parse();

    match cli.command {
        cli::Commands::Work { command } => match command {
            cli::WorkCommands::New {
                title,
                description,
                branch,
                type_name,
                target,
                assigned_to,
                tags,
                sonar_project,
            } => {
                commands::work::run(
                    title,
                    description,
                    branch,
                    type_name,
                    target,
                    assigned_to,
                    tags,
                    sonar_project,
                )
                .await?
            }
            cli::WorkCommands::Load { id, target } => commands::work::load(id, target).await?,
            cli::WorkCommands::List {
                mine,
                state,
                type_name,
                max,
            } => commands::work::list(mine, state, type_name, max).await?,
        },
        cli::Commands::Task { command } => match command {
            cli::TaskCommands::Hold { stash, force, stay } => {
                commands::task::hold(stash, force, stay).await?
            }
            cli::TaskCommands::Update {
                title,
                state,
                description,
                assigned_to,
                tags,
            } => commands::task::update(title, state, description, assigned_to, tags).await?,
            cli::TaskCommands::Complete => commands::task::complete().await?,
            cli::TaskCommands::Sync { rebase, check } => {
                commands::task::sync(rebase, check).await?
            }
        },
        cli::Commands::Pr { command } => match command {
            cli::PrCommands::Show { id } => commands::pr::show(id).await?,
            cli::PrCommands::Update {
                title,
                description,
                publish,
                status,
                add_reviewer,
            } => commands::pr::update(title, description, publish, status, add_reviewer).await?,
            cli::PrCommands::Merge {
                strategy,
                delete_source_branch,
                bypass_policy,
            } => commands::pr::merge(strategy, delete_source_branch, bypass_policy).await?,
            cli::PrCommands::Review { id } => commands::pr::review(id).await?,
        },
        cli::Commands::Pipeline { command } => match command {
            cli::PipelineCommands::Run { id } => commands::pipeline::run(id).await?,
            cli::PipelineCommands::Status { run_id, watch } => {
                commands::pipeline::status(run_id, watch).await?
            }
        },
        cli::Commands::Todo { command } => match command {
            cli::TodoCommands::Show { all, detail } => commands::todo::show(all, detail).await?,
            cli::TodoCommands::New {
                title,
                description,
                assigned_to,
                pick,
            } => commands::todo::new(title, description, assigned_to, pick).await?,
            cli::TodoCommands::Pick { reference } => commands::todo::pick(reference).await?,
            cli::TodoCommands::Complete { reference } => {
                commands::todo::complete(reference).await?
            }
            cli::TodoCommands::Reopen { reference } => commands::todo::reopen(reference).await?,
            cli::TodoCommands::Update {
                reference,
                title,
                description,
                assigned_to,
                state,
            } => commands::todo::update(reference, title, description, assigned_to, state).await?,
            cli::TodoCommands::Next { pick } => commands::todo::next(pick).await?,
        },
        cli::Commands::Context {
            only_wi,
            only_pr,
            only_git,
            only_pipeline,
        } => commands::context::run(only_wi, only_pr, only_git, only_pipeline).await?,
        cli::Commands::Commit {
            message,
            all,
            amend,
            docs_message,
            no_docs,
        } => commands::commit::run(message, all, amend, docs_message, no_docs).await?,
        cli::Commands::Push { force, no_docs } => commands::push::run(force, no_docs).await?,
        cli::Commands::Sync {
            message,
            docs_message,
        } => commands::sync::run(message, docs_message).await?,
        cli::Commands::Sonar {
            project,
            severity,
            max,
        } => commands::sonar::run(project, severity, max).await?,
        cli::Commands::Plumbing(cmd) => match cmd {
            cli::PlumbingCommands::Git { command } => match command {
                cli::GitPlumbingCommands::BranchCurrent => {
                    commands::plumbing::git::branch_current().await?
                }
            },
            cli::PlumbingCommands::Ado { command } => match command {
                cli::AdoPlumbingCommands::WiGet { id } => {
                    commands::plumbing::ado::wi_get(id).await?
                }
            },
        },
    }

    Ok(())
}
