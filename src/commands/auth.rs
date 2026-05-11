use crate::auth::token_store::AccountMeta;
use crate::auth::{app_config, device_flow, token_store};
use anyhow::{anyhow, Result};
use reqwest::Client;

pub async fn login(account: &str) -> Result<()> {
    let client_id = app_config::client_id().ok_or_else(|| {
        anyhow!(
            "GitHub App Client ID not configured.\n\
             Set the GITHUB_CLIENT_ID environment variable or compile with it set."
        )
    })?;

    println!(
        "Authenticating with GitHub (Device Flow) for account '{}'...",
        account
    );

    let token = device_flow::run_device_flow(&client_id).await?;
    token_store::save(account, &token)?;

    let username = fetch_github_username(&token.access_token).await;
    let username = username.as_deref().unwrap_or("unknown");

    token_store::save_account_meta(&AccountMeta {
        alias: account.to_string(),
        username: username.to_string(),
        expires_at: token.expires_at,
    })?;

    println!("Authenticated as @{} (account: '{}')", username, account);
    Ok(())
}

pub async fn logout(account: &str) -> Result<()> {
    token_store::delete(account)?;
    token_store::remove_account_meta(account)?;
    println!("Logged out account '{}'.", account);
    Ok(())
}

pub async fn status(account: &str) -> Result<()> {
    let stored = match token_store::load(account)? {
        Some(t) => t,
        None => {
            println!("Account '{}' is not logged in.", account);
            println!("Run `fm auth login --account {}` to authenticate.", account);
            return Ok(());
        }
    };

    if stored.is_expired() {
        println!("Token for account '{}' is expired.", account);
        println!(
            "Run `fm auth login --account {}` to re-authenticate.",
            account
        );
        return Ok(());
    }

    match fetch_github_username(&stored.access_token).await {
        Some(name) => {
            print!("Account '{}' logged in as @{}", account, name);
            match stored.seconds_until_expiry() {
                Some(0) => println!(" (token expired)"),
                Some(secs) => {
                    let hours = secs / 3600;
                    let minutes = (secs % 3600) / 60;
                    println!(" (expires in {}h {}m)", hours, minutes);
                }
                None => println!(" (no expiry set)"),
            }
        }
        None => {
            println!("Token for account '{}' is invalid or expired.", account);
            println!(
                "Run `fm auth login --account {}` to re-authenticate.",
                account
            );
        }
    }

    Ok(())
}

pub async fn list() -> Result<()> {
    let accounts = token_store::list_accounts()?;
    if accounts.is_empty() {
        println!("No accounts stored. Run `fm auth login` to authenticate.");
        return Ok(());
    }
    println!("Stored GitHub accounts:");
    for meta in &accounts {
        println!(
            "  {} — @{} ({})",
            meta.alias,
            meta.username,
            meta.expiry_summary()
        );
    }
    Ok(())
}

async fn fetch_github_username(token: &str) -> Option<String> {
    let client = Client::builder().user_agent("flow-manager").build().ok()?;
    let resp = client
        .get("https://api.github.com/user")
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: serde_json::Value = resp.json().await.ok()?;
    body["login"].as_str().map(|s| s.to_string())
}
