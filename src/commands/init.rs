use anyhow::{bail, Result};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

const DEFAULT_CONFIG_FILE: &str = "fm.toml";

const CONFIG_TEMPLATE: &str = r#"# Flow Manager configuration
# See: https://github.com/mdnmdn/flow-manager

[provider]
# Provider type: "ado" | "github" | "gitlab"
type = "ado"

[provider.ado]
url = "https://dev.azure.com/your-org"
project = "your-project"
pat = "YOUR_PAT_HERE"

# [provider.github]
# token = "YOUR_TOKEN_HERE"
# owner = "your-org"
# repo = "your-repo"

[fm]
merge_strategy = "squash"
default_target = "main"
default_wi_type = "User Story"
submodules = []

# [sonar]
# url = "https://sonar.example.com"
# token = "YOUR_SONAR_TOKEN_HERE"
"#;

// ── Discovery types ────────────────────────────────────────────────────────

#[derive(Debug)]
enum DiscoveredProvider {
    Ado {
        url: String,
        project: String,
        pat: String,
    },
    GitHub {
        token: String,
        owner: String,
        repo: String,
    },
    GitLab {
        token: String,
        namespace: String,
        project_id: Option<u64>,
    },
}

#[derive(Debug, Default)]
struct Discovered {
    provider: Option<DiscoveredProvider>,
    sonar_url: Option<String>,
    sonar_token: Option<String>,
    submodules: Vec<String>,
}

// ── .env parser ────────────────────────────────────────────────────────────

fn parse_env_file(path: &str) -> HashMap<String, String> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };
    let mut map = HashMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let key = k.trim().to_string();
            let val = v.trim().trim_matches('"').trim_matches('\'').to_string();
            if !key.is_empty() {
                map.insert(key, val);
            }
        }
    }
    map
}

fn get(env: &HashMap<String, String>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| env.get(*k))
        .filter(|v| !v.is_empty())
        .cloned()
}

// ── Git remote detection ───────────────────────────────────────────────────

fn parse_gitmodules(path: &str) -> Vec<String> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            line.strip_prefix("path")
                .and_then(|s| s.trim_start().strip_prefix('='))
                .map(|v| v.trim().to_string())
        })
        .collect()
}

fn git_remote_url() -> Option<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;
    if output.status.success() {
        let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !url.is_empty() {
            return Some(url);
        }
    }
    None
}

fn is_git_repo() -> bool {
    Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

// Parse `https://dev.azure.com/{org}/{project}/_git/{repo}`
// or    `https://{org}@dev.azure.com/{org}/{project}/_git/{repo}`
// or    `git@ssh.dev.azure.com:v3/{org}/{project}/{repo}`
// or    `https://{org}.visualstudio.com/{project}/_git/{repo}`
fn parse_ado_remote(url: &str) -> Option<(String, String)> {
    if url.contains("visualstudio.com") {
        // Org is the subdomain: https://{org}.visualstudio.com/{project}/...
        let after_https = url.trim_start_matches("https://").trim_start_matches("http://");
        let org = after_https.split('.').next()?;
        let path_start = url.find("visualstudio.com/")? + "visualstudio.com/".len();
        let path = &url[path_start..];
        let project = path.split('/').next().filter(|s| !s.is_empty())?;
        return Some((
            format!("https://{}.visualstudio.com", org),
            project.to_string(),
        ));
    }

    if !url.contains("azure.com") {
        return None;
    }

    // SSH: git@ssh.dev.azure.com:v3/{org}/{project}/{repo}
    let path = if url.starts_with("git@") {
        let colon_pos = url.find(':')?;
        url[colon_pos + 1..].trim_start_matches("v3/")
    } else {
        // HTTPS: https://[user@]dev.azure.com/{org}/{project}/_git/{repo}
        let pos = url.find("azure.com/")?;
        &url[pos + "azure.com/".len()..]
    };

    // {org}/{project}/... → parts[0]=org, parts[1]=project
    let parts: Vec<&str> = path.trim_end_matches('/').split('/').collect();
    if parts.len() >= 2 && !parts[0].is_empty() {
        let org = parts[0];
        let project = parts[1];
        return Some((format!("https://dev.azure.com/{}", org), project.to_string()));
    }
    None
}

// Parse `https://github.com/{owner}/{repo}` or `git@github.com:{owner}/{repo}`
fn parse_github_remote(url: &str) -> Option<(String, String)> {
    if !url.contains("github.com") {
        return None;
    }
    let path = if let Some(pos) = url.find("github.com/") {
        &url[pos + "github.com/".len()..]
    } else if let Some(colon_pos) = url.rfind(':') {
        &url[colon_pos + 1..]
    } else {
        return None;
    };
    let path = path.trim_end_matches(".git");
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() >= 2 {
        return Some((parts[0].to_string(), parts[1].to_string()));
    }
    None
}

// Parse `https://gitlab.com/{namespace}/{project}` or `git@gitlab.com:{namespace}/{project}`
fn parse_gitlab_remote(url: &str) -> Option<(String, String)> {
    if !url.contains("gitlab.com") && !url.contains("gitlab.") {
        return None;
    }
    let path = if let Some(pos) = url.find(".com/") {
        &url[pos + ".com/".len()..]
    } else if let Some(colon_pos) = url.rfind(':') {
        &url[colon_pos + 1..]
    } else {
        return None;
    };
    let path = path.trim_end_matches(".git");
    let parts: Vec<&str> = path.splitn(2, '/').collect();
    if parts.len() == 2 {
        return Some((parts[0].to_string(), parts[1].to_string()));
    }
    None
}

// ── Discovery logic ────────────────────────────────────────────────────────

fn discover_config(env: &HashMap<String, String>, remote_url: Option<&str>) -> Discovered {
    let sonar_url = get(env, &["SONAR_URL", "SONAR_HOST_URL"]);
    let sonar_token = get(env, &["SONAR_TOKEN"]);
    let submodules = parse_gitmodules(".gitmodules");
    let mut d = Discovered {
        sonar_url,
        sonar_token,
        submodules,
        provider: None,
    };

    if let Some(remote) = remote_url {
        if let Some((url, project)) = parse_ado_remote(remote) {
            let pat = get(env, &["ADO_PAT", "ADO_TOKEN"]).unwrap_or_default();
            let project = get(env, &["ADO_PROJECT", "ADO_PROJECT_NAME"]).unwrap_or(project);
            d.provider = Some(DiscoveredProvider::Ado { url, project, pat });
        } else if let Some((owner, repo)) = parse_github_remote(remote) {
            let token = get(env, &["GITHUB_TOKEN", "GH_TOKEN"]).unwrap_or_default();
            d.provider = Some(DiscoveredProvider::GitHub { token, owner, repo });
        } else if let Some((namespace, _repo)) = parse_gitlab_remote(remote) {
            let token = get(env, &["GITLAB_TOKEN", "CI_JOB_TOKEN"]).unwrap_or_default();
            let project_id = get(env, &["GITLAB_PROJECT_ID"])
                .and_then(|v| v.parse::<u64>().ok());
            d.provider = Some(DiscoveredProvider::GitLab { token, namespace, project_id });
        }
    } // end remote block

    // If remote gave no hint, fall back to env keys
    if d.provider.is_none() {
        if get(env, &["ADO_PAT", "ADO_TOKEN", "ADO_URL"]).is_some() {
            let url = get(env, &["ADO_URL"]).unwrap_or_else(|| "https://dev.azure.com/your-org".to_string());
            let project = get(env, &["ADO_PROJECT", "ADO_PROJECT_NAME"]).unwrap_or_default();
            let pat = get(env, &["ADO_PAT", "ADO_TOKEN"]).unwrap_or_default();
            d.provider = Some(DiscoveredProvider::Ado { url, project, pat });
        } else if get(env, &["GITHUB_TOKEN", "GH_TOKEN"]).is_some() {
            let token = get(env, &["GITHUB_TOKEN", "GH_TOKEN"]).unwrap_or_default();
            let owner = get(env, &["GITHUB_OWNER", "GITHUB_ORG"]).unwrap_or_default();
            let repo = get(env, &["GITHUB_REPO"]).unwrap_or_default();
            d.provider = Some(DiscoveredProvider::GitHub { token, owner, repo });
        } else if get(env, &["GITLAB_TOKEN"]).is_some() {
            let token = get(env, &["GITLAB_TOKEN"]).unwrap_or_default();
            let namespace = get(env, &["GITLAB_NAMESPACE"]).unwrap_or_default();
            let project_id = get(env, &["GITLAB_PROJECT_ID"])
                .and_then(|v| v.parse::<u64>().ok());
            d.provider = Some(DiscoveredProvider::GitLab { token, namespace, project_id });
        }
    }

    d
}

// ── TOML generation ────────────────────────────────────────────────────────

fn or_placeholder<'a>(val: &'a str, ph: &'a str) -> &'a str {
    if val.is_empty() { ph } else { val }
}

fn build_toml(d: &Discovered, remote_url: Option<&str>) -> String {
    let mut out = String::from("# Flow Manager configuration — generated by `fm init --discover`\n");

    if let Some(remote) = remote_url {
        out.push_str(&format!("# Detected git remote: {}\n", remote));
    }
    out.push('\n');

    match &d.provider {
        Some(DiscoveredProvider::Ado { url, project, pat }) => {
            out.push_str("[provider]\ntype = \"ado\"\n\n");
            out.push_str("[provider.ado]\n");
            out.push_str(&format!("url     = \"{}\"\n", or_placeholder(url, "https://dev.azure.com/your-org")));
            out.push_str(&format!("project = \"{}\"\n", or_placeholder(project, "your-project")));
            out.push_str(&format!("pat     = \"{}\"\n", or_placeholder(pat, "YOUR_PAT_HERE")));
        }
        Some(DiscoveredProvider::GitHub { token, owner, repo }) => {
            out.push_str("[provider]\ntype = \"github\"\n\n");
            out.push_str("[provider.github]\n");
            out.push_str(&format!("token = \"{}\"\n", or_placeholder(token, "YOUR_TOKEN_HERE")));
            out.push_str(&format!("owner = \"{}\"\n", or_placeholder(owner, "your-org")));
            out.push_str(&format!("repo  = \"{}\"\n", or_placeholder(repo, "your-repo")));
        }
        Some(DiscoveredProvider::GitLab { token, namespace, project_id }) => {
            out.push_str("[provider]\ntype = \"gitlab\"\n\n");
            out.push_str("[provider.gitlab]\n");
            out.push_str(&format!("token     = \"{}\"\n", or_placeholder(token, "YOUR_TOKEN_HERE")));
            out.push_str(&format!("namespace = \"{}\"\n", or_placeholder(namespace, "your-group/your-project")));
            if let Some(id) = project_id {
                out.push_str(&format!("project_id = {}\n", id));
            }
        }
        None => {
            out.push_str("# Could not detect provider — fill in manually\n");
            out.push_str("[provider]\ntype = \"ado\"\n\n");
            out.push_str("[provider.ado]\n");
            out.push_str("url     = \"https://dev.azure.com/your-org\"\n");
            out.push_str("project = \"your-project\"\n");
            out.push_str("pat     = \"YOUR_PAT_HERE\"\n");
        }
    }

    out.push('\n');
    out.push_str("[fm]\n");
    out.push_str("merge_strategy = \"squash\"\n");
    out.push_str("default_target = \"main\"\n");
    out.push_str("default_wi_type = \"User Story\"\n");
    if d.submodules.is_empty() {
        out.push_str("submodules = []\n");
    } else {
        let list = d.submodules
            .iter()
            .map(|s| format!("\"{}\"", s))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("submodules = [{}]\n", list));
    }

    if d.sonar_url.is_some() || d.sonar_token.is_some() {
        out.push('\n');
        out.push_str("[sonar]\n");
        if let Some(url) = &d.sonar_url {
            out.push_str(&format!("url   = \"{}\"\n", url));
        }
        if let Some(token) = &d.sonar_token {
            out.push_str(&format!("token = \"{}\"\n", token));
        }
    }

    out
}

// ── Entry point ────────────────────────────────────────────────────────────

pub async fn run(path: Option<String>, discover: bool) -> Result<()> {
    let file_path = path.as_deref().unwrap_or(DEFAULT_CONFIG_FILE);

    if Path::new(file_path).exists() {
        bail!(
            "Config file '{}' already exists. Remove it first or specify a different path with --path.",
            file_path
        );
    }

    if !discover {
        std::fs::write(file_path, CONFIG_TEMPLATE)?;
        println!("Created config file: {}", file_path);
        println!("Edit it to set your provider credentials before running other commands.");
        return Ok(());
    }

    // Discovery mode
    let env_file = ".env";
    let env = parse_env_file(env_file);
    if !env.is_empty() {
        println!("Read {} keys from {}", env.keys().count(), env_file);
    } else {
        println!("No .env file found (or empty) — relying on git remote detection.");
    }

    let in_git = is_git_repo();
    let remote_url = if in_git {
        let url = git_remote_url();
        if let Some(ref u) = url {
            println!("Detected git remote: {}", u);
        } else {
            println!("Git repo found but no 'origin' remote.");
        }
        url
    } else {
        println!("Not inside a git repository.");
        None
    };

    let discovered = discover_config(&env, remote_url.as_deref());

    match &discovered.provider {
        Some(DiscoveredProvider::Ado { .. }) => println!("Provider detected: Azure DevOps"),
        Some(DiscoveredProvider::GitHub { .. }) => println!("Provider detected: GitHub"),
        Some(DiscoveredProvider::GitLab { .. }) => println!("Provider detected: GitLab"),
        None => println!("Provider: could not detect — template placeholders used"),
    }
    if discovered.sonar_url.is_some() || discovered.sonar_token.is_some() {
        println!("SonarQube config detected.");
    }
    if !discovered.submodules.is_empty() {
        println!("Submodules detected: {}", discovered.submodules.join(", "));
    }

    let toml = build_toml(&discovered, remote_url.as_deref());

    std::fs::write(file_path, &toml)?;
    println!("Created config file: {}", file_path);

    // Warn about any remaining placeholders
    if toml.contains("YOUR_PAT_HERE")
        || toml.contains("YOUR_TOKEN_HERE")
        || toml.contains("your-org")
        || toml.contains("your-project")
    {
        println!("Some fields could not be discovered — edit {} to fill in the placeholders.", file_path);
    }

    Ok(())
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_env_file ───────────────────────────────────────────────────────

    fn write_tmp(content: &str) -> (std::path::PathBuf, TempDir) {
        let dir = std::env::temp_dir().join(format!("fm_test_{}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(".env");
        std::fs::write(&path, content).unwrap();
        (path, TempDir(dir))
    }

    struct TempDir(std::path::PathBuf);
    impl Drop for TempDir {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
    }

    #[test]
    fn env_parses_key_value_pairs() {
        let (path, _guard) = write_tmp(
            "ADO_PAT=secret\nADO_URL=https://dev.azure.com/org\n# comment\n\nSODAR_TOKEN=x\n",
        );
        let env = parse_env_file(path.to_str().unwrap());
        assert_eq!(env.get("ADO_PAT").unwrap(), "secret");
        assert_eq!(env.get("ADO_URL").unwrap(), "https://dev.azure.com/org");
        assert!(!env.contains_key("# comment"));
    }

    #[test]
    fn env_strips_quotes() {
        let (path, _guard) = write_tmp("TOKEN=\"my-token\"\nOTHER='val'\n");
        let env = parse_env_file(path.to_str().unwrap());
        assert_eq!(env.get("TOKEN").unwrap(), "my-token");
        assert_eq!(env.get("OTHER").unwrap(), "val");
    }

    #[test]
    fn env_returns_empty_for_missing_file() {
        let env = parse_env_file("/tmp/nonexistent_fm_test_file.env");
        assert!(env.is_empty());
    }

    // ── parse_ado_remote ─────────────────────────────────────────────────────

    #[test]
    fn ado_https_url() {
        let (url, project) =
            parse_ado_remote("https://dev.azure.com/contoso/myproject/_git/myrepo").unwrap();
        assert_eq!(url, "https://dev.azure.com/contoso");
        assert_eq!(project, "myproject");
    }

    #[test]
    fn ado_https_url_with_user() {
        let (url, project) =
            parse_ado_remote("https://contoso@dev.azure.com/contoso/myproject/_git/myrepo")
                .unwrap();
        assert_eq!(url, "https://dev.azure.com/contoso");
        assert_eq!(project, "myproject");
    }

    #[test]
    fn ado_ssh_url() {
        let (url, project) =
            parse_ado_remote("git@ssh.dev.azure.com:v3/contoso/myproject/myrepo").unwrap();
        assert_eq!(url, "https://dev.azure.com/contoso");
        assert_eq!(project, "myproject");
    }

    #[test]
    fn ado_visualstudio_url() {
        let (url, project) =
            parse_ado_remote("https://contoso.visualstudio.com/myproject/_git/myrepo").unwrap();
        assert_eq!(url, "https://contoso.visualstudio.com");
        assert_eq!(project, "myproject");
    }

    #[test]
    fn ado_rejects_github_url() {
        assert!(parse_ado_remote("https://github.com/owner/repo.git").is_none());
    }

    // ── parse_github_remote ──────────────────────────────────────────────────

    #[test]
    fn github_https_url() {
        let (owner, repo) =
            parse_github_remote("https://github.com/myorg/myrepo.git").unwrap();
        assert_eq!(owner, "myorg");
        assert_eq!(repo, "myrepo");
    }

    #[test]
    fn github_ssh_url() {
        let (owner, repo) = parse_github_remote("git@github.com:myorg/myrepo.git").unwrap();
        assert_eq!(owner, "myorg");
        assert_eq!(repo, "myrepo");
    }

    #[test]
    fn github_rejects_gitlab_url() {
        assert!(parse_github_remote("https://gitlab.com/group/project.git").is_none());
    }

    // ── parse_gitlab_remote ──────────────────────────────────────────────────

    #[test]
    fn gitlab_https_url() {
        let (namespace, project) =
            parse_gitlab_remote("https://gitlab.com/mygroup/myproject.git").unwrap();
        assert_eq!(namespace, "mygroup");
        assert_eq!(project, "myproject");
    }

    #[test]
    fn gitlab_ssh_url() {
        let (namespace, project) =
            parse_gitlab_remote("git@gitlab.com:mygroup/myproject.git").unwrap();
        assert_eq!(namespace, "mygroup");
        assert_eq!(project, "myproject");
    }

    // ── discover_config ──────────────────────────────────────────────────────

    fn env_from(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn discover_ado_from_env() {
        let env = env_from(&[
            ("ADO_PAT", "mypat"),
            ("ADO_URL", "https://dev.azure.com/myorg"),
            ("ADO_PROJECT", "myproj"),
        ]);
        let d = discover_config(&env, None);
        match d.provider.unwrap() {
            DiscoveredProvider::Ado { url, project, pat } => {
                assert_eq!(url, "https://dev.azure.com/myorg");
                assert_eq!(project, "myproj");
                assert_eq!(pat, "mypat");
            }
            other => panic!("unexpected provider: {:?}", other),
        }
    }

    #[test]
    fn discover_github_from_env() {
        let env = env_from(&[
            ("GITHUB_TOKEN", "ghp_abc"),
            ("GITHUB_OWNER", "myorg"),
            ("GITHUB_REPO", "myrepo"),
        ]);
        let d = discover_config(&env, None);
        match d.provider.unwrap() {
            DiscoveredProvider::GitHub { token, owner, repo } => {
                assert_eq!(token, "ghp_abc");
                assert_eq!(owner, "myorg");
                assert_eq!(repo, "myrepo");
            }
            other => panic!("unexpected provider: {:?}", other),
        }
    }

    #[test]
    fn discover_remote_overrides_env_for_provider() {
        // Even with ADO env vars, a GitHub remote should win
        let env = env_from(&[("GITHUB_TOKEN", "ghp_abc")]);
        let d = discover_config(&env, Some("https://github.com/myorg/myrepo.git"));
        match d.provider.unwrap() {
            DiscoveredProvider::GitHub { owner, repo, .. } => {
                assert_eq!(owner, "myorg");
                assert_eq!(repo, "myrepo");
            }
            other => panic!("unexpected provider: {:?}", other),
        }
    }

    #[test]
    fn discover_ado_remote_with_env_pat() {
        // Remote gives org/project; env fills in the PAT
        let env = env_from(&[("ADO_PAT", "mypat")]);
        let d = discover_config(
            &env,
            Some("https://dev.azure.com/contoso/myproject/_git/myrepo"),
        );
        match d.provider.unwrap() {
            DiscoveredProvider::Ado { url, project, pat } => {
                assert_eq!(url, "https://dev.azure.com/contoso");
                assert_eq!(project, "myproject");
                assert_eq!(pat, "mypat");
            }
            other => panic!("unexpected provider: {:?}", other),
        }
    }

    #[test]
    fn discover_sonar_from_env() {
        let env = env_from(&[
            ("SONAR_URL", "https://sonar.example.com"),
            ("SONAR_TOKEN", "squ_abc"),
        ]);
        let d = discover_config(&env, None);
        assert_eq!(d.sonar_url.unwrap(), "https://sonar.example.com");
        assert_eq!(d.sonar_token.unwrap(), "squ_abc");
    }

    #[test]
    fn discover_sonar_host_url_alias() {
        let env = env_from(&[("SONAR_HOST_URL", "https://sonar.internal")]);
        let d = discover_config(&env, None);
        assert_eq!(d.sonar_url.unwrap(), "https://sonar.internal");
    }

    #[test]
    fn discover_no_provider_returns_none() {
        let env = env_from(&[("SONAR_TOKEN", "tok")]);
        let d = discover_config(&env, None);
        assert!(d.provider.is_none());
    }

    // ── build_toml ───────────────────────────────────────────────────────────

    #[test]
    fn toml_ado_provider() {
        let d = Discovered {
            provider: Some(DiscoveredProvider::Ado {
                url: "https://dev.azure.com/myorg".into(),
                project: "myproj".into(),
                pat: "mypat".into(),
            }),
            sonar_url: None,
            sonar_token: None,
            submodules: vec![],
        };
        let out = build_toml(&d, None);
        assert!(out.contains("type = \"ado\""));
        assert!(out.contains("url     = \"https://dev.azure.com/myorg\""));
        assert!(out.contains("project = \"myproj\""));
        assert!(out.contains("pat     = \"mypat\""));
        assert!(!out.contains("[sonar]"));
    }

    #[test]
    fn toml_github_provider() {
        let d = Discovered {
            provider: Some(DiscoveredProvider::GitHub {
                token: "ghp_abc".into(),
                owner: "myorg".into(),
                repo: "myrepo".into(),
            }),
            sonar_url: None,
            sonar_token: None,
            submodules: vec![],
        };
        let out = build_toml(&d, None);
        assert!(out.contains("type = \"github\""));
        assert!(out.contains("token = \"ghp_abc\""));
        assert!(out.contains("owner = \"myorg\""));
        assert!(out.contains("repo  = \"myrepo\""));
    }

    #[test]
    fn toml_includes_sonar_when_present() {
        let d = Discovered {
            provider: None,
            sonar_url: Some("https://sonar.example.com".into()),
            sonar_token: Some("squ_xyz".into()),
            submodules: vec![],
        };
        let out = build_toml(&d, None);
        assert!(out.contains("[sonar]"));
        assert!(out.contains("url   = \"https://sonar.example.com\""));
        assert!(out.contains("token = \"squ_xyz\""));
    }

    #[test]
    fn toml_includes_remote_comment() {
        let d = Discovered { provider: None, sonar_url: None, sonar_token: None, submodules: vec![] };
        let out = build_toml(&d, Some("https://github.com/org/repo.git"));
        assert!(out.contains("# Detected git remote: https://github.com/org/repo.git"));
    }

    #[test]
    fn toml_always_includes_fm_section() {
        let d = Discovered { provider: None, sonar_url: None, sonar_token: None, submodules: vec![] };
        let out = build_toml(&d, None);
        assert!(out.contains("[fm]"));
        assert!(out.contains("merge_strategy"));
        assert!(out.contains("default_target"));
        assert!(out.contains("submodules = []"));
    }

    #[test]
    fn toml_lists_discovered_submodules() {
        let d = Discovered {
            provider: None,
            sonar_url: None,
            sonar_token: None,
            submodules: vec!["_docs".into(), "vendor/lib".into()],
        };
        let out = build_toml(&d, None);
        assert!(out.contains("submodules = [\"_docs\", \"vendor/lib\"]"));
    }

    // ── parse_gitmodules ─────────────────────────────────────────────────────

    #[test]
    fn gitmodules_parses_paths() {
        let (path, _guard) = write_tmp(
            "[submodule \"_docs\"]\n\tpath = _docs\n\turl = https://github.com/org/docs\n\
             [submodule \"vendor/lib\"]\n\tpath = vendor/lib\n\turl = https://github.com/org/lib\n",
        );
        let subs = parse_gitmodules(path.to_str().unwrap());
        assert_eq!(subs, vec!["_docs", "vendor/lib"]);
    }

    #[test]
    fn gitmodules_returns_empty_for_missing_file() {
        let subs = parse_gitmodules("/tmp/nonexistent_fm_.gitmodules");
        assert!(subs.is_empty());
    }
}
