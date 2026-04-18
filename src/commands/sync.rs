use crate::commands::{commit, push};
use anyhow::Result;

pub async fn run(message: Option<String>, docs_message: Option<String>) -> Result<()> {
    commit::run(message, true, false, docs_message, false).await?;
    push::run(false, false).await?;
    Ok(())
}
