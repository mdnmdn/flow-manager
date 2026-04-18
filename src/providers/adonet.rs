use async_trait::async_trait;
use crate::providers::{IssueTracker, VCSProvider, PipelineProvider};
use crate::core::models::{WorkItem, PullRequest, Pipeline, PipelineRun, Repository};
use anyhow::{Result, anyhow};
use reqwest::{Client, header};
use base64::{Engine as _, engine::general_purpose};
use serde_json::{json, Value};

use crate::core::config::AdoConfig;

pub struct AzureDevOpsProvider {
    client: Client,
    organization_url: String,
    project: String,
}

impl AzureDevOpsProvider {
    pub fn new(config: &AdoConfig) -> Result<Self> {
        let mut headers = header::HeaderMap::new();
        let auth = format!(":{}", config.pat);
        let encoded = general_purpose::STANDARD.encode(auth);
        let mut auth_value = header::HeaderValue::from_str(&format!("Basic {}", encoded))?;
        auth_value.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, auth_value);

        let client = Client::builder()
            .default_headers(headers)
            .build()?;

        Ok(Self {
            client,
            organization_url: config.url.trim_end_matches('/').to_string(),
            project: config.project.clone(),
        })
    }

    fn v(&self, url: &str) -> String {
        if url.contains('?') {
            format!("{}&api-version=7.1", url)
        } else {
            format!("{}?api-version=7.1", url)
        }
    }

    fn base_api_url(&self) -> String {
        format!("{}/{}/_apis", self.organization_url, self.project)
    }

    fn normalize_ref(&self, branch: &str) -> String {
        if branch.starts_with("refs/") {
            branch.to_string()
        } else {
            format!("refs/heads/{}", branch)
        }
    }
}

#[async_trait]
impl IssueTracker for AzureDevOpsProvider {
    async fn get_work_item(&self, id: i32) -> Result<WorkItem> {
        let url = self.v(&format!("{}/wit/workitems/{}", self.base_api_url(), id));
        let resp = self.client.get(url).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!("Failed to get work item {}: {}", id, resp.text().await?));
        }

        let body: Value = resp.json().await?;
        let fields = &body["fields"];

        Ok(WorkItem {
            id,
            title: fields["System.Title"].as_str().unwrap_or_default().to_string(),
            work_item_type: fields["System.WorkItemType"].as_str().unwrap_or_default().to_string(),
            state: fields["System.State"].as_str().unwrap_or_default().to_string(),
        })
    }

    async fn create_work_item(&self, title: &str, work_item_type: &str) -> Result<WorkItem> {
        let url = self.v(&format!("{}/wit/workitems/${}", self.base_api_url(), work_item_type));

        let patch = json!([
            {
                "op": "add",
                "path": "/fields/System.Title",
                "value": title
            }
        ]);

        let resp = self.client.post(url)
            .header(header::CONTENT_TYPE, "application/json-patch+json")
            .json(&patch)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!("Failed to create work item: {}", resp.text().await?));
        }

        let body: Value = resp.json().await?;
        let id = body["id"].as_i64().ok_or_else(|| anyhow!("No ID in response"))? as i32;
        let fields = &body["fields"];

        Ok(WorkItem {
            id,
            title: fields["System.Title"].as_str().unwrap_or_default().to_string(),
            work_item_type: fields["System.WorkItemType"].as_str().unwrap_or_default().to_string(),
            state: fields["System.State"].as_str().unwrap_or_default().to_string(),
        })
    }

    async fn update_work_item(&self, id: i32, state: &str) -> Result<WorkItem> {
        let url = self.v(&format!("{}/wit/workitems/{}", self.base_api_url(), id));

        let patch = json!([
            {
                "op": "add",
                "path": "/fields/System.State",
                "value": state
            }
        ]);

        let resp = self.client.patch(url)
            .header(header::CONTENT_TYPE, "application/json-patch+json")
            .json(&patch)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!("Failed to update work item {}: {}", id, resp.text().await?));
        }

        let body: Value = resp.json().await?;
        let fields = &body["fields"];

        Ok(WorkItem {
            id,
            title: fields["System.Title"].as_str().unwrap_or_default().to_string(),
            work_item_type: fields["System.WorkItemType"].as_str().unwrap_or_default().to_string(),
            state: fields["System.State"].as_str().unwrap_or_default().to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Server, ServerGuard};

    async fn setup_mock_server() -> (ServerGuard, AzureDevOpsProvider) {
        let server = Server::new_async().await;
        let config = AdoConfig {
            url: server.url(),
            project: "test-project".to_string(),
            pat: "test-pat".to_string(),
        };
        let provider = AzureDevOpsProvider::new(&config).unwrap();
        (server, provider)
    }

    #[tokio::test]
    async fn test_get_work_item() {
        let (mut server, provider) = setup_mock_server().await;

        let mock = server.mock("GET", "/test-project/_apis/wit/workitems/123?api-version=7.1")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(json!({
                "id": 123,
                "fields": {
                    "System.Title": "Test Title",
                    "System.WorkItemType": "User Story",
                    "System.State": "New"
                }
            }).to_string())
            .create_async().await;

        let result = provider.get_work_item(123).await.unwrap();
        assert_eq!(result.id, 123);
        assert_eq!(result.title, "Test Title");
        assert_eq!(result.work_item_type, "User Story");
        assert_eq!(result.state, "New");

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_create_work_item() {
        let (mut server, provider) = setup_mock_server().await;

        let mock = server.mock("POST", "/test-project/_apis/wit/workitems/$User%20Story?api-version=7.1")
            .match_header("content-type", "application/json-patch+json")
            .match_body(mockito::Matcher::Json(json!([
                {
                    "op": "add",
                    "path": "/fields/System.Title",
                    "value": "New Task"
                }
            ])))
            .with_status(201)
            .with_body(json!({
                "id": 456,
                "fields": {
                    "System.Title": "New Task",
                    "System.WorkItemType": "User Story",
                    "System.State": "New"
                }
            }).to_string())
            .create_async().await;

        let result = provider.create_work_item("New Task", "User Story").await.unwrap();
        assert_eq!(result.id, 456);
        assert_eq!(result.title, "New Task");

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_pull_request() {
        let (mut server, provider) = setup_mock_server().await;

        let mock = server.mock("GET", "/test-project/_apis/git/repositories/my-repo/pullrequests/789?api-version=7.1")
            .with_status(200)
            .with_body(json!({
                "pullRequestId": 789,
                "title": "PR Title",
                "status": "active",
                "sourceRefName": "refs/heads/feature",
                "targetRefName": "refs/heads/main",
                "isDraft": false
            }).to_string())
            .create_async().await;

        let result = provider.get_pull_request("my-repo", 789).await.unwrap();
        assert_eq!(result.id, 789);
        assert_eq!(result.title, "PR Title");

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_create_branch() {
        let (mut server, provider) = setup_mock_server().await;

        let mock_get_ref = server.mock("GET", "/test-project/_apis/git/repositories/my-repo/refs?filter=heads/main&api-version=7.1")
            .with_status(200)
            .with_body(json!({
                "value": [{
                    "objectId": "sha123"
                }]
            }).to_string())
            .create_async().await;

        let mock_create_ref = server.mock("POST", "/test-project/_apis/git/repositories/my-repo/refs?api-version=7.1")
            .match_body(mockito::Matcher::Json(json!([
                {
                    "name": "refs/heads/new-branch",
                    "oldObjectId": "0000000000000000000000000000000000000000",
                    "newObjectId": "sha123"
                }
            ])))
            .with_status(201)
            .create_async().await;

        provider.create_branch("my-repo", "new-branch", "main").await.unwrap();

        mock_get_ref.assert_async().await;
        mock_create_ref.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_pipelines() {
        let (mut server, provider) = setup_mock_server().await;

        let mock = server.mock("GET", "/test-project/_apis/pipelines?api-version=7.1")
            .with_status(200)
            .with_body(json!({
                "value": [
                    { "id": 1, "name": "Pipe 1", "folder": "\\" },
                    { "id": 2, "name": "Pipe 2", "folder": "\\nested" }
                ]
            }).to_string())
            .create_async().await;

        let result = provider.list_pipelines().await.unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "Pipe 1");

        mock.assert_async().await;
    }
}

#[async_trait]
impl VCSProvider for AzureDevOpsProvider {
    async fn get_pull_request(&self, repository: &str, id: i32) -> Result<PullRequest> {
        let url = self.v(&format!("{}/git/repositories/{}/pullrequests/{}", self.base_api_url(), repository, id));
        let resp = self.client.get(url).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!("Failed to get PR {}: {}", id, resp.text().await?));
        }

        let body: Value = resp.json().await?;

        Ok(PullRequest {
            id,
            title: body["title"].as_str().unwrap_or_default().to_string(),
            status: body["status"].as_str().unwrap_or_default().to_string(),
            source_branch: body["sourceRefName"].as_str().unwrap_or_default().to_string(),
            target_branch: body["targetRefName"].as_str().unwrap_or_default().to_string(),
            is_draft: body["isDraft"].as_bool().unwrap_or_default(),
        })
    }

    async fn create_pull_request(&self, repository: &str, title: &str, source: &str, target: &str) -> Result<PullRequest> {
        let url = self.v(&format!("{}/git/repositories/{}/pullrequests", self.base_api_url(), repository));

        let body = json!({
            "sourceRefName": self.normalize_ref(source),
            "targetRefName": self.normalize_ref(target),
            "title": title,
            "isDraft": true
        });

        let resp = self.client.post(url).json(&body).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!("Failed to create PR: {}", resp.text().await?));
        }

        let body: Value = resp.json().await?;
        let id = body["pullRequestId"].as_i64().ok_or_else(|| anyhow!("No pullRequestId in response"))? as i32;

        Ok(PullRequest {
            id,
            title: body["title"].as_str().unwrap_or_default().to_string(),
            status: body["status"].as_str().unwrap_or_default().to_string(),
            source_branch: body["sourceRefName"].as_str().unwrap_or_default().to_string(),
            target_branch: body["targetRefName"].as_str().unwrap_or_default().to_string(),
            is_draft: body["isDraft"].as_bool().unwrap_or_default(),
        })
    }

    async fn create_branch(&self, repository: &str, name: &str, source: &str) -> Result<()> {
        let normalized_source = self.normalize_ref(source);
        let normalized_name = self.normalize_ref(name);

        // Get source branch objectId
        let refs_url = self.v(&format!("{}/git/repositories/{}/refs?filter={}", self.base_api_url(), repository, normalized_source.trim_start_matches("refs/")));
        let resp = self.client.get(refs_url).send().await?;

        if !resp.status().is_success() {
             return Err(anyhow!("Failed to find source branch {}: {}", source, resp.text().await?));
        }

        let body: Value = resp.json().await?;
        let source_sha = body["value"][0]["objectId"].as_str()
            .ok_or_else(|| anyhow!("Could not find objectId for source branch {}", source))?;

        let create_url = self.v(&format!("{}/git/repositories/{}/refs", self.base_api_url(), repository));
        let body = json!([
            {
                "name": normalized_name,
                "oldObjectId": "0000000000000000000000000000000000000000",
                "newObjectId": source_sha
            }
        ]);

        let resp = self.client.post(create_url).json(&body).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!("Failed to create branch {}: {}", name, resp.text().await?));
        }

        Ok(())
    }

    async fn get_repository(&self, name: &str) -> Result<Repository> {
        let url = self.v(&format!("{}/git/repositories/{}", self.base_api_url(), name));
        let resp = self.client.get(url).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!("Failed to get repository {}: {}", name, resp.text().await?));
        }

        let body: Value = resp.json().await?;

        Ok(Repository {
            id: body["id"].as_str().unwrap_or_default().to_string(),
            name: body["name"].as_str().unwrap_or_default().to_string(),
            project_id: body["project"]["id"].as_str().unwrap_or_default().to_string(),
            default_branch: body["defaultBranch"].as_str().map(|s| s.to_string()),
        })
    }
}

#[async_trait]
impl PipelineProvider for AzureDevOpsProvider {
    async fn list_pipelines(&self) -> Result<Vec<Pipeline>> {
        let url = self.v(&format!("{}/pipelines", self.base_api_url()));
        let resp = self.client.get(url).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!("Failed to list pipelines: {}", resp.text().await?));
        }

        let body: Value = resp.json().await?;
        let items = body["value"].as_array().ok_or_else(|| anyhow!("No value array in response"))?;

        let mut pipelines = Vec::new();
        for item in items {
            pipelines.push(Pipeline {
                id: item["id"].as_i64().unwrap_or_default() as i32,
                name: item["name"].as_str().unwrap_or_default().to_string(),
                folder: item["folder"].as_str().unwrap_or_default().to_string(),
            });
        }

        Ok(pipelines)
    }

    async fn run_pipeline(&self, pipeline_id: i32, branch: &str) -> Result<PipelineRun> {
        let url = self.v(&format!("{}/pipelines/{}/runs", self.base_api_url(), pipeline_id));

        let body = json!({
            "resources": {
                "repositories": {
                    "self": {
                        "refName": self.normalize_ref(branch)
                    }
                }
            }
        });

        let resp = self.client.post(url).json(&body).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!("Failed to run pipeline {}: {}", pipeline_id, resp.text().await?));
        }

        let body: Value = resp.json().await?;

        Ok(PipelineRun {
            id: body["id"].as_i64().unwrap_or_default() as i32,
            status: body["state"].as_str().unwrap_or_default().to_string(),
            result: body["result"].as_str().map(|s| s.to_string()),
            url: body["_links"]["web"]["href"].as_str().unwrap_or_default().to_string(),
        })
    }

    async fn get_pipeline_run(&self, pipeline_id: i32, run_id: i32) -> Result<PipelineRun> {
        let url = self.v(&format!("{}/pipelines/{}/runs/{}", self.base_api_url(), pipeline_id, run_id));
        let resp = self.client.get(url).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!("Failed to get pipeline run {}/{}: {}", pipeline_id, run_id, resp.text().await?));
        }

        let body: Value = resp.json().await?;

        Ok(PipelineRun {
            id: body["id"].as_i64().unwrap_or_default() as i32,
            status: body["state"].as_str().unwrap_or_default().to_string(),
            result: body["result"].as_str().map(|s| s.to_string()),
            url: body["_links"]["web"]["href"].as_str().unwrap_or_default().to_string(),
        })
    }
}
