use crate::auth::token_store::StoredToken;
use anyhow::{anyhow, bail, Result};
use reqwest::Client;
use serde::Deserialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::sleep;

const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";

#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[allow(dead_code)]
    expires_in: u64,
    interval: u64,
}

#[derive(Debug, Deserialize)]
struct TokenPollResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    error: Option<String>,
    #[allow(dead_code)]
    token_type: Option<String>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Runs the full GitHub Device Flow, prompts the user, and returns a stored token on success.
pub async fn run_device_flow(client_id: &str) -> Result<StoredToken> {
    let client = build_client()?;

    // Step 1: Request device + user codes
    let resp = client
        .post(DEVICE_CODE_URL)
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "client_id": client_id,
            "scope": "repo"
        }))
        .send()
        .await
        .map_err(|e| anyhow!("Failed to request device code: {}", e))?;

    if !resp.status().is_success() {
        bail!(
            "GitHub returned HTTP {} for device code request",
            resp.status()
        );
    }

    let code_resp: DeviceCodeResponse = resp
        .json()
        .await
        .map_err(|e| anyhow!("Failed to parse device code response: {}", e))?;

    // Step 2: Prompt the user
    println!();
    println!("  Open this URL in your browser:");
    println!("  {}", code_resp.verification_uri);
    println!();
    println!("  Enter this code: {}", code_resp.user_code);
    println!();
    println!("  Waiting for authorization...");

    // Attempt to open the browser automatically
    let _ = open::that(&code_resp.verification_uri);

    // Step 3: Poll for the access token
    let mut poll_interval = code_resp.interval;
    loop {
        sleep(Duration::from_secs(poll_interval)).await;

        let poll_resp = client
            .post(ACCESS_TOKEN_URL)
            .header("Accept", "application/json")
            .json(&serde_json::json!({
                "client_id": client_id,
                "device_code": code_resp.device_code,
                "grant_type": "urn:ietf:params:oauth:grant-type:device_code"
            }))
            .send()
            .await
            .map_err(|e| anyhow!("Failed to poll for token: {}", e))?;

        let result: TokenPollResponse = poll_resp
            .json()
            .await
            .map_err(|e| anyhow!("Failed to parse token poll response: {}", e))?;

        if let Some(access_token) = result.access_token {
            let expires_at = result.expires_in.map(|secs| now_secs() + secs);
            return Ok(StoredToken {
                access_token,
                refresh_token: result.refresh_token,
                expires_at,
            });
        }

        match result.error.as_deref() {
            Some("authorization_pending") => continue,
            Some("slow_down") => {
                poll_interval += 5;
                continue;
            }
            Some("expired_token") => {
                bail!("Device code expired. Run `fm auth login` again.");
            }
            Some("access_denied") => bail!("Authorization was denied."),
            Some(other) => bail!("Authorization failed: {}", other),
            None => bail!("Unexpected empty response from GitHub authorization server"),
        }
    }
}

/// Refreshes an existing token using the refresh_token grant.
pub async fn refresh_token(client_id: &str, refresh_token: &str) -> Result<StoredToken> {
    let client = build_client()?;

    let resp = client
        .post(ACCESS_TOKEN_URL)
        .header("Accept", "application/json")
        .json(&serde_json::json!({
            "client_id": client_id,
            "grant_type": "refresh_token",
            "refresh_token": refresh_token
        }))
        .send()
        .await
        .map_err(|e| anyhow!("Failed to send refresh request: {}", e))?;

    let result: TokenPollResponse = resp
        .json()
        .await
        .map_err(|e| anyhow!("Failed to parse refresh response: {}", e))?;

    let access_token = result.access_token.ok_or_else(|| {
        anyhow!(
            "No access_token in refresh response (error: {:?})",
            result.error
        )
    })?;

    let expires_at = result.expires_in.map(|secs| now_secs() + secs);

    Ok(StoredToken {
        access_token,
        refresh_token: result.refresh_token,
        expires_at,
    })
}

fn build_client() -> Result<Client> {
    Client::builder()
        .user_agent("flow-manager")
        .build()
        .map_err(|e| anyhow!("Failed to build HTTP client: {}", e))
}
