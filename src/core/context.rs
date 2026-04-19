use crate::core::models::WorkItemId;
use anyhow::Result;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub enum Context {
    Baseline {
        branch: String,
    },
    Activity {
        branch: String,
        wi_id: WorkItemId,
        wi_type: String, // feature or fix
    },
}

static BRANCH_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(feature|fix)/([A-Z]+-\d+|\d+)-(.+)$").unwrap());

pub struct ContextManager;

impl ContextManager {
    pub fn detect(branch: &str) -> Context {
        if let Some(caps) = BRANCH_RE.captures(branch) {
            return Context::Activity {
                branch: branch.to_string(),
                wi_id: WorkItemId(caps[2].to_string()),
                wi_type: caps[1].to_string(),
            };
        }
        Context::Baseline {
            branch: branch.to_string(),
        }
    }

    pub fn resolve_id(input: &str) -> IdResolution {
        if let Some(caps) = BRANCH_RE.captures(input) {
            return IdResolution::WorkItem(WorkItemId(caps[2].to_string()));
        }

        if input.starts_with("w-") || input.starts_with("wi-") || input.starts_with("w") {
            let id = input
                .trim_start_matches("wi-")
                .trim_start_matches("w-")
                .trim_start_matches('w');
            if !id.is_empty() {
                return IdResolution::WorkItem(WorkItemId(id.to_string()));
            }
        }
        if input.starts_with("pr-") || input.starts_with("p-") {
            let id = input.trim_start_matches("pr-").trim_start_matches("p-");
            if !id.is_empty() {
                return IdResolution::PullRequest(id.to_string());
            }
        }

        // Generic ID could be work item or PR, but we'll default to Ambiguous WorkItem for now
        // if it matches the pattern of a possible ID.
        if !input.is_empty() && input.chars().all(|c| c.is_alphanumeric() || c == '-') {
            return IdResolution::Ambiguous(WorkItemId(input.to_string()));
        }

        IdResolution::Unknown(input.to_string())
    }
}

pub enum IdResolution {
    WorkItem(WorkItemId),
    PullRequest(String),
    Ambiguous(WorkItemId),
    Unknown(String),
}

impl ContextManager {
    pub fn derive_branch_name(wi_id: &WorkItemId, title: &str, wi_type: &str) -> String {
        let slug = title.to_lowercase().replace(' ', "-");
        let prefix = if wi_type == "Bug" || wi_type == "fix" {
            "fix"
        } else {
            "feature"
        };
        format!("{}/{}-{}", prefix, wi_id, slug)
    }
}

pub struct OutputFormatter;

impl OutputFormatter {
    pub fn format<T: Serialize>(data: &T, format: &str, template: Option<&str>) -> Result<String> {
        match format {
            "json" => Ok(serde_json::to_string_pretty(data)?),
            "yaml" => Ok(serde_yaml::to_string(data)?),
            _ => {
                if let Some(tpl) = template {
                    Self::render_markdown(tpl, data)
                } else {
                    Ok("No template provided for markdown output".to_string())
                }
            }
        }
    }

    fn render_markdown<T: Serialize>(template: &str, data: &T) -> Result<String> {
        let val = serde_json::to_value(data)?;
        let mut rendered = template.to_string();

        if let Some(obj) = val.as_object() {
            for (k, v) in obj {
                let placeholder = format!("{{{{{}}}}}", k);
                let replacement = match v {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    _ => serde_json::to_string(v)?,
                };
                rendered = rendered.replace(&placeholder, &replacement);
            }
        }

        Ok(rendered)
    }
}
