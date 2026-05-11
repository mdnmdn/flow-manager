use anyhow::{anyhow, Result};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

const SERVICE: &str = "flow-manager";
const USER: &str = "github-app";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StoredToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<u64>, // Unix timestamp seconds
}

impl StoredToken {
    pub fn is_expired(&self) -> bool {
        if let Some(exp) = self.expires_at {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            now >= exp
        } else {
            false
        }
    }

    pub fn expires_soon(&self) -> bool {
        if let Some(exp) = self.expires_at {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            now + 1800 >= exp
        } else {
            false
        }
    }

    pub fn seconds_until_expiry(&self) -> Option<u64> {
        let exp = self.expires_at?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if exp > now { Some(exp - now) } else { Some(0) }
    }
}

pub fn save(token: &StoredToken) -> Result<()> {
    let entry = Entry::new(SERVICE, USER).map_err(|e| anyhow!("Keychain error: {}", e))?;
    let json = serde_json::to_string(token)?;
    entry
        .set_password(&json)
        .map_err(|e| anyhow!("Failed to save token to keychain: {}", e))
}

pub fn load() -> Result<Option<StoredToken>> {
    let entry = Entry::new(SERVICE, USER).map_err(|e| anyhow!("Keychain error: {}", e))?;
    match entry.get_password() {
        Ok(json) => Ok(Some(serde_json::from_str(&json)?)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(anyhow!("Failed to load token from keychain: {}", e)),
    }
}

pub fn delete() -> Result<()> {
    let entry = Entry::new(SERVICE, USER).map_err(|e| anyhow!("Keychain error: {}", e))?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(anyhow!("Failed to delete token from keychain: {}", e)),
    }
}

/// Loads the token and silently refreshes it if expiring within 30 minutes.
/// Returns None if no token is stored.
/// Returns Err if the token is expired and refresh failed (user must re-login).
pub async fn load_valid_token(client_id: &str) -> Result<Option<String>> {
    let token = match load()? {
        Some(t) => t,
        None => return Ok(None),
    };

    if !token.is_expired() && !token.expires_soon() {
        return Ok(Some(token.access_token));
    }

    // Try to refresh using the refresh_token
    if let Some(ref refresh) = token.refresh_token {
        match crate::auth::device_flow::refresh_token(client_id, refresh).await {
            Ok(new_token) => {
                save(&new_token)?;
                return Ok(Some(new_token.access_token));
            }
            Err(_) => {}
        }
    }

    if token.is_expired() {
        Err(anyhow!(
            "GitHub App token expired. Run `fm auth login` to re-authenticate."
        ))
    } else {
        Ok(Some(token.access_token))
    }
}
