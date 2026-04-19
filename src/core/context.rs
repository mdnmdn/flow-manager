use crate::core::models::WorkItemId;
use anyhow::Result;
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

pub struct ContextManager;

impl ContextManager {
    pub fn detect(branch: &str) -> Context {
        if branch.starts_with("feature/") || branch.starts_with("fix/") {
            let parts: Vec<&str> = branch.splitn(2, '/').collect();
            if parts.len() == 2 {
                let segment = parts[1];
                // Matches: PROJ-123-slug  OR  123-slug  (alphanumeric project keys)
                if let Some(wi_id) = Self::extract_wi_id_from_segment(segment) {
                    return Context::Activity {
                        branch: branch.to_string(),
                        wi_id,
                        wi_type: parts[0].to_string(),
                    };
                }
            }
        }
        Context::Baseline {
            branch: branch.to_string(),
        }
    }

    fn extract_wi_id_from_segment(segment: &str) -> Option<WorkItemId> {
        // Try alphanumeric Jira-style key first: PROJ-123-rest-of-slug
        // Pattern: one or more uppercase letters, dash, digits, then optionally dash + rest
        let mut chars = segment.chars().peekable();
        let mut prefix = String::new();

        // Collect uppercase letters
        while chars.peek().map(|c| c.is_ascii_uppercase()).unwrap_or(false) {
            prefix.push(chars.next().unwrap());
        }

        if !prefix.is_empty() && chars.peek() == Some(&'-') {
            chars.next(); // consume '-'
            let mut digits = String::new();
            while chars.peek().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                digits.push(chars.next().unwrap());
            }
            if !digits.is_empty() {
                // Must be followed by '-' or end of string
                if chars.peek().map(|c| *c == '-').unwrap_or(true) {
                    let key = format!("{}-{}", prefix, digits);
                    return Some(WorkItemId(key));
                }
            }
        }

        // Fall back to plain integer: 123-slug
        let first_part = segment.split('-').next()?;
        if let Ok(_) = first_part.parse::<i64>() {
            return Some(WorkItemId(first_part.to_string()));
        }

        None
    }

    pub fn resolve_id(input: &str) -> IdResolution {
        // Plain integer
        if let Ok(_) = input.parse::<i64>() {
            return IdResolution::Ambiguous(WorkItemId(input.to_string()));
        }
        // w-123 / wi-123 / w123
        if input.starts_with("wi-") || input.starts_with("w-") || input.starts_with('w') {
            let numeric = input
                .trim_start_matches("wi-")
                .trim_start_matches("w-")
                .trim_start_matches('w');
            if let Ok(_) = numeric.parse::<i64>() {
                return IdResolution::WorkItem(WorkItemId(numeric.to_string()));
            }
        }
        // pr-123 / p-123
        if input.starts_with("pr-") || input.starts_with("p-") {
            let numeric = input.trim_start_matches("pr-").trim_start_matches("p-");
            if let Ok(_) = numeric.parse::<i64>() {
                return IdResolution::PullRequest(numeric.to_string());
            }
        }
        // feature/123-slug or fix/PROJ-123-slug
        if input.starts_with("feature/") || input.starts_with("fix/") {
            let parts: Vec<&str> = input.splitn(2, '/').collect();
            if parts.len() == 2 {
                if let Some(wi_id) = Self::extract_wi_id_from_segment(parts[1]) {
                    return IdResolution::WorkItem(wi_id);
                }
            }
        }
        // Jira-style key directly: PROJ-123
        {
            let mut chars = input.chars().peekable();
            let mut prefix = String::new();
            while chars.peek().map(|c| c.is_ascii_uppercase()).unwrap_or(false) {
                prefix.push(chars.next().unwrap());
            }
            if !prefix.is_empty() && chars.peek() == Some(&'-') {
                chars.next();
                let rest: String = chars.collect();
                if let Ok(_) = rest.parse::<i64>() {
                    return IdResolution::WorkItem(WorkItemId(input.to_string()));
                }
            }
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
        let slug = title
            .to_lowercase()
            .replace(' ', "-")
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-')
            .collect::<String>();
        let prefix = if wi_type == "Bug" || wi_type == "fix" {
            "fix"
        } else {
            "feature"
        };
        format!("{}/{}-{}", prefix, wi_id.as_str(), slug)
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
