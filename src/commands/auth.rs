use crate::auth::{app_config, device_flow, token_store};
use anyhow::{anyhow, Result};
use reqwest::Client;

pub async fn login() -> Result<()> {
    let client_id = app_config::client_id().ok_or_else(|| {
        anyhow!(
            "GitHub App Client ID not configured.\n\
             Set the GITHUB_CLIENT_ID environment variable or compile with it set."
        )
    })?;

    println!("Authenticating with GitHub (Device Flow)...");

    let token = device_flow::run_device_flow(&client_id).await?;
    token_store::save(&token)?;

    // Verify the token by fetching the authenticated user
    let login_name = fetch_github_username(&token.access_token).await;
    match login_name {
        Some(name) => println!("Authenticated as @{}", name),
        None => println!("Authenticated successfully."),
    }

    Ok(())
}

pub async fn logout() -> Result<()> {
    token_store::delete()?;
    println!("Logged out from GitHub App.");
    Ok(())
}

pub async fn status() -> Result<()> {
    let stored = match token_store::load()? {
        Some(t) => t,
        None => {
            println!("Not logged in via GitHub App.");
            println!("Run `fm auth login` to authenticate.");
            return Ok(());
        }
    };

    if stored.is_expired() {
        println!("GitHub App token is expired.");
        println!("Run `fm auth login` to re-authenticate.");
        return Ok(());
    }

    // Verify token is still valid with GitHub
    match fetch_github_username(&stored.access_token).await {
        Some(name) => {
            print!("Logged in as @{}", name);
            match stored.seconds_until_expiry() {
                Some(0) => println!(" (token expired)"),
                Some(secs) => {
                    let hours = secs / 3600;
                    let minutes = (secs % 3600) / 60;
                    println!(" (token expires in {}h {}m)", hours, minutes);
                }
                None => println!(" (no expiry set)"),
            }
        }
        None => {
            println!("GitHub App token is invalid or expired.");
            println!("Run `fm auth login` to re-authenticate.");
        }
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
