use fm::cli;
use fm::commands;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let cli = cli::parse();

    match cli.command {
        cli::Commands::Task { command } => match command {
            cli::TaskCommands::New {
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
            cli::TaskCommands::Load {
                id,
                target,
                init,
                branch,
            } => commands::work::load(id, target, init, branch).await?,
            cli::TaskCommands::List {
                mine,
                state,
                type_name,
                max,
            } => commands::work::list(mine, state, type_name, max).await?,
            cli::TaskCommands::Show {
                id,
                no_comments,
                compact,
            } => commands::work::show(id.unwrap_or_default(), !no_comments, compact).await?,
            cli::TaskCommands::Comment { message } => commands::task::comment(message).await?,
            cli::TaskCommands::Hold { force, stay } => commands::task::hold(force, stay).await?,
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
            cli::PrCommands::Show {
                id,
                out,
                include_project_context,
            } => commands::pr::show(id, out, include_project_context).await?,
            cli::PrCommands::Thread { command } => match command {
                cli::PrThreadCommands::List { id, status } => {
                    commands::pr::thread::list(id, status).await?
                }
                cli::PrThreadCommands::Reply {
                    thread_id,
                    message,
                    pr,
                    resolve,
                } => commands::pr::thread::reply(pr, thread_id, message, resolve).await?,
                cli::PrThreadCommands::Resolve {
                    thread_ids,
                    pr,
                    comment,
                } => commands::pr::thread::resolve(pr, thread_ids, comment).await?,
            },
            cli::PrCommands::Feedback { command } => match command {
                cli::PrFeedbackCommands::Validate { file, pr, format } => {
                    commands::pr::feedback::validate(file, pr, format).await?
                }
                cli::PrFeedbackCommands::Apply {
                    file,
                    pr,
                    format,
                    dry_run,
                    force,
                } => commands::pr::feedback::apply(file, pr, format, dry_run, force).await?,
            },
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
            cli::PrCommands::Comment { id, message } => commands::pr::comment(id, message).await?,
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
            only_task,
            only_pr,
            only_git,
            only_pipeline,
            task_comments,
        } => {
            commands::context::run(only_task, only_pr, only_git, only_pipeline, task_comments)
                .await?
        }
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
        cli::Commands::Sonar { command } => {
            let config = fm::core::config::Config::load()?;
            let sonar_config = config
                .sonar
                .ok_or_else(|| anyhow::anyhow!("SonarQube not configured"))?;
            commands::sonar::run(command, &sonar_config).await?
        }
        cli::Commands::Doctor { fix } => commands::doctor::run(fix).await?,
        cli::Commands::Init { path, discover } => commands::init::run(path, discover).await?,
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
        cli::Commands::Version => {
            println!(
                "fm version {}",
                option_env!("FM_VERSION").unwrap_or(env!("CARGO_PKG_VERSION"))
            );
        }
    }

    Ok(())
}
