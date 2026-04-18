use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub enum Context {
    Baseline {
        branch: String,
    },
    Activity {
        branch: String,
        wi_id: i32,
        wi_type: String, // feature or fix
    },
}

pub struct ContextManager;

impl ContextManager {
    pub fn detect(branch: &str) -> Context {
        if branch.starts_with("feature/") || branch.starts_with("fix/") {
            let parts: Vec<&str> = branch.split('/').collect();
            if parts.len() == 2 {
                let wi_parts: Vec<&str> = parts[1].split('-').collect();
                if let Ok(id) = wi_parts[0].parse::<i32>() {
                    return Context::Activity {
                        branch: branch.to_string(),
                        wi_id: id,
                        wi_type: parts[0].to_string(),
                    };
                }
            }
        }
        Context::Baseline {
            branch: branch.to_string(),
        }
    }

    pub fn resolve_id(input: &str) -> IdResolution {
        if let Ok(id) = input.parse::<i32>() {
            return IdResolution::Ambiguous(id);
        }
        if input.starts_with("w-") || input.starts_with("wi-") || input.starts_with("w") {
            let numeric = input
                .trim_start_matches("wi-")
                .trim_start_matches("w-")
                .trim_start_matches('w');
            if let Ok(id) = numeric.parse::<i32>() {
                return IdResolution::WorkItem(id);
            }
        }
        if input.starts_with("pr-") || input.starts_with("p-") {
            let numeric = input.trim_start_matches("pr-").trim_start_matches("p-");
            if let Ok(id) = numeric.parse::<i32>() {
                return IdResolution::PullRequest(id);
            }
        }
        if input.starts_with("feature/") || input.starts_with("fix/") {
            let parts: Vec<&str> = input.split('/').collect();
            if parts.len() == 2 {
                let wi_parts: Vec<&str> = parts[1].split('-').collect();
                if let Ok(id) = wi_parts[0].parse::<i32>() {
                    return IdResolution::WorkItem(id);
                }
            }
        }
        IdResolution::Unknown(input.to_string())
    }
}

pub enum IdResolution {
    WorkItem(i32),
    PullRequest(i32),
    Ambiguous(i32),
    Unknown(String),
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
