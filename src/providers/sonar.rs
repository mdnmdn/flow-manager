use crate::core::config::SonarConfig;
use crate::core::models::QualityIssue;
use crate::providers::QualityProvider;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use base64::Engine;
use reqwest::{header, Client};
use serde_json::Value;

pub struct SonarProvider {
    client: Client,
    url: String,
}

impl SonarProvider {
    pub fn new(config: &SonarConfig) -> Result<Self> {
        let mut headers = header::HeaderMap::new();
        let credentials = format!("{}:", config.token);
        let encoded = base64::engine::general_purpose::STANDARD.encode(credentials);
        let auth_value = header::HeaderValue::from_str(&format!("Basic {}", encoded))?;
        headers.insert(header::AUTHORIZATION, auth_value);

        let client = Client::builder().default_headers(headers).build()?;

        Ok(Self {
            client,
            url: config.url.trim_end_matches('/').to_string(),
        })
    }
}

#[async_trait]
impl QualityProvider for SonarProvider {
    async fn get_open_issues(
        &self,
        project_key: &str,
        severity: Option<&str>,
    ) -> Result<Vec<QualityIssue>> {
        let mut url = format!(
            "{}/api/issues/search?componentKeys={}&resolved=false",
            self.url, project_key
        );

        if let Some(sev) = severity {
            url.push_str(&format!("&severities={}", sev));
        }

        let resp = self.client.get(url).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "Failed to fetch SonarQube issues: {}",
                resp.text().await?
            ));
        }

        let body: Value = resp.json().await?;
        let issues = body["issues"]
            .as_array()
            .ok_or_else(|| anyhow!("No issues array in response"))?;

        let mut results = Vec::new();
        for issue in issues {
            let text_range = issue.get("textRange");
            let (start_line, end_line, start_offset, end_offset) = text_range
                .and_then(|tr| {
                    Some((
                        tr.get("startLine")?.as_i64().map(|v| v as i32),
                        tr.get("endLine")?.as_i64().map(|v| v as i32),
                        tr.get("startOffset")?.as_i64().map(|v| v as i32),
                        tr.get("endOffset")?.as_i64().map(|v| v as i32),
                    ))
                })
                .unwrap_or((None, None, None, None));

            results.push(QualityIssue {
                key: issue["key"].as_str().unwrap_or_default().to_string(),
                message: issue["message"].as_str().unwrap_or_default().to_string(),
                severity: issue["severity"].as_str().unwrap_or_default().to_string(),
                component: issue["component"].as_str().unwrap_or_default().to_string(),
                start_line,
                end_line,
                start_offset,
                end_offset,
            });
        }

        Ok(results)
    }
}

#[derive(Debug, Clone)]
pub struct QualityProject {
    pub key: String,
    pub name: String,
    pub visibility: String,
    pub last_analysis: Option<String>,
}

impl SonarProvider {
    pub async fn list_projects(
        &self,
        search: Option<&str>,
        favorites: bool,
    ) -> Result<Vec<QualityProject>> {
        let endpoint = if favorites {
            "/api/favorites/search?ps=500".to_string()
        } else {
            let q = search.map(|s| format!("&q={}", s)).unwrap_or_default();
            format!("/api/projects/search?ps=500{}", q)
        };

        let resp = self
            .client
            .get(format!("{}{}", self.url, endpoint))
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "Failed to fetch SonarQube projects: {}",
                resp.text().await?
            ));
        }

        let body: Value = resp.json().await?;
        let components = body["components"]
            .as_array()
            .ok_or_else(|| anyhow!("No components array in response"))?;

        let mut results = Vec::new();
        for comp in components {
            results.push(QualityProject {
                key: comp["key"].as_str().unwrap_or_default().to_string(),
                name: comp["name"].as_str().unwrap_or_default().to_string(),
                visibility: comp["visibility"].as_str().unwrap_or_default().to_string(),
                last_analysis: comp["lastAnalysisDate"].as_str().map(|s| s.to_string()),
            });
        }

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;
    use serde_json::json;

    #[tokio::test]
    async fn test_get_open_issues() {
        let mut server = Server::new_async().await;
        let config = SonarConfig {
            url: server.url(),
            token: "test-token".to_string(),
            projects: vec![],
        };
        let provider = SonarProvider::new(&config).unwrap();

        let mock = server
            .mock(
                "GET",
                "/api/issues/search?componentKeys=my-project&resolved=false",
            )
            .with_status(200)
            .with_body(
                json!({
                    "issues": [
                        {
                            "key": "issue-1",
                            "message": "Bad code",
                            "severity": "MAJOR",
                            "component": "file.rs"
                        }
                    ]
                })
                .to_string(),
            )
            .create_async()
            .await;

        let issues = provider
            .get_open_issues("my-project", None::<&str>)
            .await
            .unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].key, "issue-1");
        assert_eq!(issues[0].severity, "MAJOR");

        mock.assert_async().await;
    }
}
