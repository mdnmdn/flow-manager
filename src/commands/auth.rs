use crate::auth::token_store::AccountMeta;
use crate::auth::{app_config, device_flow, token_store};
use crate::core::config::Config;
use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::Deserialize;

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

    if let Some((owner, repo, app_id, app_name)) = configured_github_repo() {
        print_installation_status(&token.access_token, &owner, &repo, app_id, app_name).await;
    }

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

    if let Some((owner, repo, app_id, app_name)) = configured_github_repo() {
        print_installation_status(&stored.access_token, &owner, &repo, app_id, app_name).await;
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

fn configured_github_repo() -> Option<(String, String, Option<i64>, Option<String>)> {
    let cfg = Config::load().ok()?;
    let provider = cfg.provider?;
    if provider.kind != "github" {
        return None;
    }
    let gh = provider.github?;
    if gh.owner.is_empty() || gh.repo.is_empty() {
        return None;
    }
    let runtime_app_id = app_config::app_id();
    let app_id_raw = gh.app_id.as_deref().or(runtime_app_id.as_deref());
    let app_id = app_id_raw.and_then(|s| s.parse::<i64>().ok());
    let app_name = app_config::app_name().or_else(|| {
        gh.app_id.as_ref().and_then(|value| {
            if value.parse::<i64>().is_ok() {
                None
            } else {
                Some(value.clone())
            }
        })
    });
    Some((gh.owner, gh.repo, app_id, app_name))
}

#[derive(Debug, Deserialize)]
struct UserInstallationsResponse {
    installations: Vec<UserInstallation>,
}

#[derive(Debug, Deserialize)]
struct UserInstallation {
    id: i64,
    app_id: i64,
    app_slug: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InstallationRepositoriesResponse {
    repositories: Vec<RepositoryItem>,
}

#[derive(Debug, Deserialize)]
struct RepositoryItem {
    full_name: String,
}

enum InstallationState {
    Installed,
    NotInstalled,
    Unknown,
}

async fn print_installation_status(
    token: &str,
    owner: &str,
    repo: &str,
    app_id: Option<i64>,
    configured_app_name: Option<String>,
) {
    let state = check_repo_installation(token, owner, repo, app_id).await;
    let mut app_name = fetch_app_slug_from_installations(token, app_id).await;
    if app_name.is_none() {
        app_name = configured_app_name;
    }
    let full_name = format!("{}/{}", owner, repo);
    let repo_installations_url = format!(
        "https://github.com/{}/{}/settings/installations",
        owner, repo
    );
    let app_target_url = app_name.map(|name| {
        format!(
            "https://github.com/apps/{}/installations/select_target",
            name
        )
    });
    match state {
        InstallationState::Installed => {
            println!(
                "GitHub App installation check: installed for {}.",
                full_name
            );
            println!(
                "Manage app access for this repo: {}",
                repo_installations_url
            );
        }
        InstallationState::NotInstalled => {
            if let Some(url) = &app_target_url {
                println!("Authorize this app for the repo: {}", url);
            }
            println!(
                "GitHub App installation check: no installation found for {}.\nInstall the app for this repository, then run `fm auth status` again.",
                full_name
            );
            println!(
                "Repository installation settings: {}",
                repo_installations_url
            );
        }
        InstallationState::Unknown => {
            if let Some(url) = &app_target_url {
                println!("Authorize this app for the repo: {}", url);
            }
            println!(
                "GitHub App installation check: unable to verify installation for {}.",
                full_name
            );
            println!(
                "Repository installation settings: {}",
                repo_installations_url
            );
        }
    }
}

async fn fetch_app_slug_from_installations(token: &str, app_id: Option<i64>) -> Option<String> {
    let client = Client::builder().user_agent("flow-manager").build().ok()?;
    let resp = client
        .get("https://api.github.com/user/installations")
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: UserInstallationsResponse = resp.json().await.ok()?;
    if let Some(expected_app_id) = app_id {
        return body
            .installations
            .into_iter()
            .find(|i| i.app_id == expected_app_id)
            .and_then(|i| i.app_slug);
    }
    body.installations.into_iter().find_map(|i| i.app_slug)
}

async fn check_repo_installation(
    token: &str,
    owner: &str,
    repo: &str,
    app_id: Option<i64>,
) -> InstallationState {
    let client = match Client::builder().user_agent("flow-manager").build() {
        Ok(c) => c,
        Err(_) => return InstallationState::Unknown,
    };

    let list_resp = match client
        .get("https://api.github.com/user/installations")
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        _ => return InstallationState::Unknown,
    };

    let list: UserInstallationsResponse = match list_resp.json().await {
        Ok(v) => v,
        Err(_) => return InstallationState::Unknown,
    };

    let full_name = format!("{}/{}", owner, repo).to_lowercase();
    for inst in list.installations {
        if let Some(expected_app_id) = app_id {
            if inst.app_id != expected_app_id {
                continue;
            }
        }

        let repos_resp = match client
            .get(format!(
                "https://api.github.com/user/installations/{}/repositories",
                inst.id
            ))
            .header("Authorization", format!("Bearer {}", token))
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r,
            _ => continue,
        };

        let repos: InstallationRepositoriesResponse = match repos_resp.json().await {
            Ok(v) => v,
            Err(_) => continue,
        };

        if repos
            .repositories
            .iter()
            .any(|r| r.full_name.to_lowercase() == full_name)
        {
            return InstallationState::Installed;
        }
    }

    InstallationState::NotInstalled
}
