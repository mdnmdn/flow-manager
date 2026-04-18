pub async fn run(message: Option<String>, _docs_message: Option<String>) -> anyhow::Result<()> {
    // SPECIFICATION:
    // Shorthand for fm commit --all && fm push.
    //
    // PSEUDO-CODE:
    // 1. Execute Commit command with all=true.
    // 2. Execute Push command.
    println!("Scaffold: fm sync --message {:?}", message);
    Ok(())
}
