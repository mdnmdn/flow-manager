use anyhow::{anyhow, Result};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const SERVICE: &str = "flow-manager";

fn keyring_user(account: &str) -> String {
    format!("github:{}", account)
}

fn accounts_index_path() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".config")
        .join("flow-manager")
        .join("accounts.json")
}

fn tokens_dir() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home)
        .join(".config")
        .join("flow-manager")
        .join("tokens")
}

fn token_fallback_path(account: &str) -> PathBuf {
    tokens_dir().join(format!("{}.json", account))
}

// ── Token ────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct StoredToken {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<u64>, // Unix timestamp seconds
}

impl StoredToken {
    pub fn is_expired(&self) -> bool {
        if let Some(exp) = self.expires_at {
            now_secs() >= exp
        } else {
            false
        }
    }

    pub fn expires_soon(&self) -> bool {
        if let Some(exp) = self.expires_at {
            now_secs() + 1800 >= exp
        } else {
            false
        }
    }

    pub fn seconds_until_expiry(&self) -> Option<u64> {
        let exp = self.expires_at?;
        let now = now_secs();
        if exp > now {
            Some(exp - now)
        } else {
            Some(0)
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn save(account: &str, token: &StoredToken) -> Result<()> {
    let json = serde_json::to_string(token)?;

    let entry = Entry::new(SERVICE, &keyring_user(account))
        .map_err(|e| anyhow!("Keychain error: {}", e))?;

    if entry.set_password(&json).is_err() {
        save_token_fallback(account, token)?;
        return Ok(());
    }

    save_token_fallback(account, token)?;
    Ok(())
}

pub fn load(account: &str) -> Result<Option<StoredToken>> {
    let entry = Entry::new(SERVICE, &keyring_user(account))
        .map_err(|e| anyhow!("Keychain error: {}", e))?;

    match entry.get_password() {
        Ok(json) => Ok(Some(serde_json::from_str(&json)?)),
        Err(keyring::Error::NoEntry) => load_token_fallback(account),
        Err(_) => load_token_fallback(account),
    }
}

pub fn delete(account: &str) -> Result<()> {
    let entry = Entry::new(SERVICE, &keyring_user(account))
        .map_err(|e| anyhow!("Keychain error: {}", e))?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(_) => {}
    }

    delete_token_fallback(account)?;
    Ok(())
}

fn save_token_fallback(account: &str, token: &StoredToken) -> Result<()> {
    let path = token_fallback_path(account);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string(token)?;
    std::fs::write(path, json)?;
    Ok(())
}

fn load_token_fallback(account: &str) -> Result<Option<StoredToken>> {
    let path = token_fallback_path(account);
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Ok(None);
    };
    Ok(Some(serde_json::from_str(&contents)?))
}

fn delete_token_fallback(account: &str) -> Result<()> {
    let path = token_fallback_path(account);
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// Loads the token for `account` and silently refreshes if expiring within 30 minutes.
/// Returns `None` if no token is stored for this account.
/// Returns `Err` if the token is expired and refresh also failed.
pub async fn load_valid_token(account: &str, client_id: &str) -> Result<Option<String>> {
    let token = match load(account)? {
        Some(t) => t,
        None => return Ok(None),
    };

    if !token.is_expired() && !token.expires_soon() {
        return Ok(Some(token.access_token));
    }

    if let Some(ref refresh) = token.refresh_token {
        if let Ok(new_token) = crate::auth::device_flow::refresh_token(client_id, refresh).await {
            save(account, &new_token)?;
            return Ok(Some(new_token.access_token));
        }
    }

    if token.is_expired() {
        Err(anyhow!(
            "GitHub App token for account '{}' expired. Run `fm auth login --account {}` to re-authenticate.",
            account, account
        ))
    } else {
        Ok(Some(token.access_token))
    }
}

// ── Account index ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AccountMeta {
    pub alias: String,
    pub username: String,
    pub expires_at: Option<u64>,
}

impl AccountMeta {
    pub fn expiry_summary(&self) -> String {
        match self.expires_at {
            None => "no expiry".to_string(),
            Some(exp) => {
                let now = now_secs();
                if exp <= now {
                    "expired".to_string()
                } else {
                    let secs = exp - now;
                    let hours = secs / 3600;
                    let minutes = (secs % 3600) / 60;
                    format!("expires in {}h {}m", hours, minutes)
                }
            }
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct AccountIndex {
    accounts: Vec<AccountMeta>,
}

fn load_index() -> AccountIndex {
    let path = accounts_index_path();
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return AccountIndex::default();
    };
    serde_json::from_str(&contents).unwrap_or_default()
}

fn save_index(index: &AccountIndex) -> Result<()> {
    let path = accounts_index_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(index)?;
    std::fs::write(&path, json)?;
    Ok(())
}

pub fn list_accounts() -> Result<Vec<AccountMeta>> {
    Ok(load_index().accounts)
}

pub fn save_account_meta(meta: &AccountMeta) -> Result<()> {
    let mut index = load_index();
    if let Some(existing) = index.accounts.iter_mut().find(|a| a.alias == meta.alias) {
        *existing = meta.clone();
    } else {
        index.accounts.push(meta.clone());
    }
    save_index(&index)
}

pub fn remove_account_meta(alias: &str) -> Result<()> {
    let mut index = load_index();
    index.accounts.retain(|a| a.alias != alias);
    save_index(&index)
}
