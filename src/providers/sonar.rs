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
            results.push(QualityIssue {
                key: issue["key"].as_str().unwrap_or_default().to_string(),
                message: issue["message"].as_str().unwrap_or_default().to_string(),
                severity: issue["severity"].as_str().unwrap_or_default().to_string(),
                component: issue["component"].as_str().unwrap_or_default().to_string(),
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
