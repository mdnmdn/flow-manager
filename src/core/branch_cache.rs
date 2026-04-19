use crate::core::models::WorkItemId;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchCache {
    pub branch: String,
    pub wi_id: WorkItemId,
    pub wi_type: String,
}

fn repo_hash() -> String {
    let root = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| ".".to_string());

    // FNV-1a 64-bit
    let mut hash: u64 = 14695981039346656037;
    for byte in root.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    format!("{:016x}", hash)
}

fn cache_path() -> PathBuf {
    std::env::temp_dir().join(format!("fm_branch_{}.json", repo_hash()))
}

impl BranchCache {
    pub fn save(branch: &str, wi_id: &WorkItemId, wi_type: &str) {
        let cache = BranchCache {
            branch: branch.to_string(),
            wi_id: wi_id.clone(),
            wi_type: wi_type.to_string(),
        };
        if let Ok(json) = serde_json::to_string(&cache) {
            let _ = std::fs::write(cache_path(), json);
        }
    }

    /// Returns the cached entry only if `branch` matches exactly and wi_id is non-empty.
    pub fn load_for_branch(branch: &str) -> Option<BranchCache> {
        let content = std::fs::read_to_string(cache_path()).ok()?;
        let cache: BranchCache = serde_json::from_str(&content).ok()?;
        if cache.branch != branch || cache.wi_id.0.is_empty() {
            return None;
        }
        Some(cache)
    }

    pub fn clear() {
        let _ = std::fs::remove_file(cache_path());
    }
}
