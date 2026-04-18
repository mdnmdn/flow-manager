use app::cli;
use app::core::config::Config;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let _config = Config::load().ok();
    let cli = cli::parse();

    match cli.command {
        cli::Commands::Work { command } => match command {
            cli::WorkCommands::New { .. } => println!("work new"),
            cli::WorkCommands::Load { .. } => println!("work load"),
            cli::WorkCommands::List { .. } => println!("work list"),
        },
        cli::Commands::Task { command } => match command {
            cli::TaskCommands::Hold { .. } => println!("task hold"),
            cli::TaskCommands::Update { .. } => println!("task update"),
            cli::TaskCommands::Complete => println!("task complete"),
            cli::TaskCommands::Sync { .. } => println!("task sync"),
        },
        cli::Commands::Pr { command } => match command {
            cli::PrCommands::Show { .. } => println!("pr show"),
            cli::PrCommands::Update { .. } => println!("pr update"),
            cli::PrCommands::Merge { .. } => println!("pr merge"),
            cli::PrCommands::Review { .. } => println!("pr review"),
        },
        cli::Commands::Pipeline { command } => match command {
            cli::PipelineCommands::Run { .. } => println!("pipeline run"),
            cli::PipelineCommands::Status { .. } => println!("pipeline status"),
        },
        cli::Commands::Todo { command } => match command {
            cli::TodoCommands::Show { .. } => println!("todo show"),
            cli::TodoCommands::New { .. } => println!("todo new"),
            cli::TodoCommands::Pick { .. } => println!("todo pick"),
            cli::TodoCommands::Complete { .. } => println!("todo complete"),
            cli::TodoCommands::Reopen { .. } => println!("todo reopen"),
            cli::TodoCommands::Update { .. } => println!("todo update"),
            cli::TodoCommands::Next { .. } => println!("todo next"),
        },
        cli::Commands::Context { .. } => println!("context"),
        cli::Commands::Commit { .. } => println!("commit"),
        cli::Commands::Push { .. } => println!("push"),
        cli::Commands::Sync { .. } => println!("sync"),
        cli::Commands::Sonar { .. } => println!("sonar"),
        cli::Commands::Plumbing(cmd) => match cmd {
            cli::PlumbingCommands::Git { command } => match command {
                cli::GitPlumbingCommands::BranchCurrent => println!("git branch-current"),
            },
            cli::PlumbingCommands::Ado { command } => match command {
                cli::AdoPlumbingCommands::WiGet { id } => println!("ado wi-get {}", id),
            },
        },
    }

    Ok(())
}
