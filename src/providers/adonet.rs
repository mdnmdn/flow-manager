use crate::core::models::{
    MergeStrategy, Pipeline, PipelineRun, ProviderCapabilities, PullRequest, Repository, WorkItem,
    WorkItemFilter, WorkItemId,
};
use crate::providers::{IssueTracker, PipelineProvider, VCSProvider};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose, Engine as _};
use reqwest::{header, Client};
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

        let client = Client::builder().default_headers(headers).build()?;

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

impl AzureDevOpsProvider {
    fn parse_work_item(&self, body: &Value) -> Result<WorkItem> {
        let fields = &body["fields"];
        let id = WorkItemId(
            body["id"]
                .as_i64()
                .ok_or_else(|| anyhow!("No ID in work item: {:?}", body))?
                .to_string(),
        );

        let tags = fields["System.Tags"]
            .as_str()
            .map(|s| {
                s.split(';')
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty())
                    .collect()
            })
            .unwrap_or_default();

        Ok(WorkItem {
            id,
            title: fields["System.Title"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            work_item_type: fields["System.WorkItemType"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            state: fields["System.State"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            description: fields["System.Description"]
                .as_str()
                .or_else(|| fields["Microsoft.VSTS.TCM.ReproductionSteps"].as_str())
                .map(|s| s.to_string()),
            assigned_to: fields["System.AssignedTo"]["displayName"]
                .as_str()
                .or_else(|| fields["System.AssignedTo"]["uniqueName"].as_str())
                .or_else(|| fields["System.AssignedTo"].as_str())
                .map(|s| s.to_string()),
            tags,
        })
    }

    async fn get_work_items_batch(&self, ids: Vec<WorkItemId>) -> Result<Vec<WorkItem>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }

        let url = self.v(&format!("{}/wit/workitemsbatch", self.base_api_url()));
        let body = json!({
            "ids": ids.iter().map(|id: &WorkItemId| id.as_str()).collect::<Vec<&str>>(),
            "fields": [
                "System.Id",
                "System.Title",
                "System.WorkItemType",
                "System.State",
                "System.Description",
                "Microsoft.VSTS.TCM.ReproductionSteps",
                "System.AssignedTo",
                "System.Tags"
            ]
        });

        let resp = self.client.post(url).json(&body).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "Failed to fetch work items details: {}",
                resp.text().await?
            ));
        }

        let body: Value = resp.json().await?;
        let items = body["value"]
            .as_array()
            .ok_or_else(|| anyhow!("No value array in batch response"))?;

        let mut results = Vec::new();
        for item in items {
            results.push(self.parse_work_item(item)?);
        }

        Ok(results)
    }
}

#[async_trait]
impl IssueTracker for AzureDevOpsProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            draft_pull_requests: true,
            pipeline_support: true,
            work_item_hierarchy: true,
            formal_artifact_links: true,
            merge_strategies: vec![
                MergeStrategy::Squash,
                MergeStrategy::Rebase,
                MergeStrategy::RebaseMerge,
                MergeStrategy::NoFastForward,
            ],
            work_item_relations: vec![
                "System.LinkTypes.Hierarchy-Forward".to_string(),
                "System.LinkTypes.Hierarchy-Reverse".to_string(),
                "System.LinkTypes.Dependency-Forward".to_string(),
                "System.LinkTypes.Dependency-Reverse".to_string(),
                "System.LinkTypes.Related".to_string(),
            ],
        }
    }

    async fn get_work_item(&self, id: &WorkItemId) -> Result<WorkItem> {
        let url = self.v(&format!(
            "{}/wit/workitems/{}?$expand=fields",
            self.base_api_url(),
            id
        ));
        let resp = self.client.get(url).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "Failed to get work item {}: {}",
                id,
                resp.text().await?
            ));
        }

        let body: Value = resp.json().await?;
        self.parse_work_item(&body)
    }

    async fn create_work_item(
        &self,
        title: &str,
        work_item_type: &str,
        description: Option<&str>,
        assigned_to: Option<&str>,
        tags: Option<Vec<&str>>,
    ) -> Result<WorkItem> {
        let url = self.v(&format!(
            "{}/wit/workitems/${}",
            self.base_api_url(),
            work_item_type
        ));

        let mut patch = vec![json!({
            "op": "add",
            "path": "/fields/System.Title",
            "value": title
        })];

        if let Some(desc) = description {
            patch.push(json!({
                "op": "add",
                "path": "/fields/System.Description",
                "value": desc
            }));
        }

        if let Some(assignee) = assigned_to {
            patch.push(json!({
                "op": "add",
                "path": "/fields/System.AssignedTo",
                "value": assignee
            }));
        }

        if let Some(t) = tags {
            patch.push(json!({
                "op": "add",
                "path": "/fields/System.Tags",
                "value": t.join(";")
            }));
        }

        let resp = self
            .client
            .post(url)
            .header(header::CONTENT_TYPE, "application/json-patch+json")
            .json(&patch)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "Failed to create work item: {}",
                resp.text().await?
            ));
        }

        let body: Value = resp.json().await?;
        self.parse_work_item(&body)
    }

    async fn update_work_item(
        &self,
        id: &WorkItemId,
        title: Option<&str>,
        description: Option<&str>,
        assigned_to: Option<&str>,
        tags: Option<Vec<&str>>,
    ) -> Result<WorkItem> {
        let url = self.v(&format!("{}/wit/workitems/{}", self.base_api_url(), id));

        let mut patch = Vec::new();

        if let Some(t) = title {
            patch.push(json!({
                "op": "add",
                "path": "/fields/System.Title",
                "value": t
            }));
        }

        if let Some(desc) = description {
            patch.push(json!({
                "op": "add",
                "path": "/fields/System.Description",
                "value": desc
            }));
        }

        if let Some(assignee) = assigned_to {
            patch.push(json!({
                "op": "add",
                "path": "/fields/System.AssignedTo",
                "value": assignee
            }));
        }

        if let Some(t) = tags {
            patch.push(json!({
                "op": "add",
                "path": "/fields/System.Tags",
                "value": t.join(";")
            }));
        }

        if patch.is_empty() {
            return self.get_work_item(id).await;
        }

        let resp = self
            .client
            .patch(url)
            .header(header::CONTENT_TYPE, "application/json-patch+json")
            .json(&patch)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "Failed to update work item {}: {}",
                id,
                resp.text().await?
            ));
        }

        let body: Value = resp.json().await?;
        self.parse_work_item(&body)
    }

    async fn update_work_item_state(&self, id: &WorkItemId, state: &str) -> Result<WorkItem> {
        let url = self.v(&format!("{}/wit/workitems/{}", self.base_api_url(), id));

        let patch = json!([
            {
                "op": "add",
                "path": "/fields/System.State",
                "value": state
            }
        ]);

        let resp = self
            .client
            .patch(url)
            .header(header::CONTENT_TYPE, "application/json-patch+json")
            .json(&patch)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "Failed to update work item state {}: {}",
                id,
                resp.text().await?
            ));
        }

        let body: Value = resp.json().await?;
        self.parse_work_item(&body)
    }

    async fn query_work_items(&self, filter: &WorkItemFilter) -> Result<Vec<WorkItem>> {
        let mut wiql = format!(
            "SELECT [System.Id] FROM WorkItems WHERE [System.TeamProject] = '{}'",
            self.project
        );

        if let Some(state) = &filter.state {
            wiql.push_str(&format!(" AND [System.State] = '{}'", state));
        }
        if let Some(assigned_to) = &filter.assigned_to {
            if assigned_to == "@Me" {
                wiql.push_str(" AND [System.AssignedTo] = @Me");
            } else {
                wiql.push_str(&format!(" AND [System.AssignedTo] = '{}'", assigned_to));
            }
        }
        if let Some(wi_type) = &filter.work_item_type {
            wiql.push_str(&format!(" AND [System.WorkItemType] = '{}'", wi_type));
        }
        for label in &filter.labels {
            wiql.push_str(&format!(" AND [System.Tags] CONTAINS '{}'", label));
        }
        if let Some(text) = &filter.text {
            wiql.push_str(&format!(" AND [System.Title] CONTAINS '{}'", text));
        }

        wiql.push_str(" ORDER BY [System.ChangedDate] DESC");

        let url = self.v(&format!("{}/wit/wiql", self.base_api_url()));
        let body = json!({ "query": wiql });
        let resp = self.client.post(url).json(&body).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "Failed to query work items: {}",
                resp.text().await?
            ));
        }

        let body: Value = resp.json().await?;
        let work_items_refs = body["workItems"]
            .as_array()
            .ok_or_else(|| anyhow!("No workItems in response"))?;

        if work_items_refs.is_empty() {
            return Ok(vec![]);
        }

        let mut ids: Vec<WorkItemId> = work_items_refs
            .iter()
            .map(|wi| WorkItemId(wi["id"].as_i64().unwrap_or_default().to_string()))
            .collect();

        if let Some(limit) = filter.limit {
            ids.truncate(limit as usize);
        }

        self.get_work_items_batch(ids).await
    }

    async fn create_artifact_link(&self, wi_id: &WorkItemId, url: &str) -> Result<()> {
        let api_url = self.v(&format!("{}/wit/workitems/{}", self.base_api_url(), wi_id));
        let patch = json!([
            {
                "op": "add",
                "path": "/relations/-",
                "value": {
                    "rel": "ArtifactLink",
                    "url": url,
                    "attributes": {
                        "name": "Integrated in build"
                    }
                }
            }
        ]);

        let resp = self
            .client
            .patch(api_url)
            .header(header::CONTENT_TYPE, "application/json-patch+json")
            .json(&patch)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "Failed to link artifact to work item: {}",
                resp.text().await?
            ));
        }

        Ok(())
    }

    async fn link_work_items(
        &self,
        source_id: &WorkItemId,
        target_id: &WorkItemId,
        relation: &str,
    ) -> Result<()> {
        let api_url = self.v(&format!(
            "{}/wit/workitems/{}",
            self.base_api_url(),
            source_id
        ));

        // We need the target work item URL
        let target_wi_url = format!("{}/wit/workitems/{}", self.base_api_url(), target_id);

        let patch = json!([
            {
                "op": "add",
                "path": "/relations/-",
                "value": {
                    "rel": relation,
                    "url": target_wi_url,
                }
            }
        ]);

        let resp = self
            .client
            .patch(api_url)
            .header(header::CONTENT_TYPE, "application/json-patch+json")
            .json(&patch)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(anyhow!("Failed to link work items: {}", resp.text().await?));
        }

        Ok(())
    }

    async fn get_child_work_items(
        &self,
        id: &WorkItemId,
        work_item_type: Option<&str>,
    ) -> Result<Vec<WorkItem>> {
        let url = self.v(&format!(
            "{}/wit/workitems/{}?$expand=relations",
            self.base_api_url(),
            id
        ));
        let resp = self.client.get(url).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "Failed to get work item relations: {}",
                resp.text().await?
            ));
        }

        let body: Value = resp.json().await?;
        let relations = body["relations"].as_array();

        if let Some(rels) = relations {
            let child_ids: Vec<WorkItemId> = rels
                .iter()
                .filter(|r| r["rel"] == "System.LinkTypes.Hierarchy-Forward")
                .filter_map(|r| {
                    r["url"]
                        .as_str()
                        .and_then(|url| Some(WorkItemId(url.split('/').next_back()?.to_string())))
                })
                .collect();

            if child_ids.is_empty() {
                return Ok(vec![]);
            }

            let children = self.get_work_items_batch(child_ids).await?;

            if let Some(wi_type) = work_item_type {
                return Ok(children
                    .into_iter()
                    .filter(|wi| wi.work_item_type == wi_type)
                    .collect());
            }

            return Ok(children);
        }

        Ok(vec![])
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
    async fn test_delete_branch() {
        let (mut server, provider) = setup_mock_server().await;

        let mock_get_ref = server.mock("GET", "/test-project/_apis/git/repositories/my-repo/refs?filter=heads/to-delete&api-version=7.1")
            .with_status(200)
            .with_body(json!({
                "value": [{
                    "objectId": "sha-to-delete"
                }]
            }).to_string())
            .create_async().await;

        let mock_delete_ref = server
            .mock(
                "POST",
                "/test-project/_apis/git/repositories/my-repo/refs?api-version=7.1",
            )
            .match_body(mockito::Matcher::Json(json!([
                {
                    "name": "refs/heads/to-delete",
                    "oldObjectId": "sha-to-delete",
                    "newObjectId": "0000000000000000000000000000000000000000"
                }
            ])))
            .with_status(201)
            .create_async()
            .await;

        provider
            .delete_branch("my-repo", "to-delete")
            .await
            .unwrap();

        mock_get_ref.assert_async().await;
        mock_delete_ref.assert_async().await;
    }

    #[tokio::test]
    async fn test_update_work_item_generic() {
        let (mut server, provider) = setup_mock_server().await;

        let mock = server
            .mock(
                "PATCH",
                "/test-project/_apis/wit/workitems/123?api-version=7.1",
            )
            .match_header("content-type", "application/json-patch+json")
            .match_body(mockito::Matcher::Json(json!([
                {
                    "op": "add",
                    "path": "/fields/System.Title",
                    "value": "Updated Title"
                }
            ])))
            .with_status(200)
            .with_body(
                json!({
                    "id": 123,
                    "fields": {
                        "System.Title": "Updated Title",
                        "System.WorkItemType": "User Story",
                        "System.State": "New"
                    }
                })
                .to_string(),
            )
            .create_async()
            .await;

        let result = provider
            .update_work_item(
                &WorkItemId::from_int(123),
                Some("Updated Title"),
                None,
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(result.title, "Updated Title");

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_update_work_item_state() {
        let (mut server, provider) = setup_mock_server().await;

        let mock = server
            .mock(
                "PATCH",
                "/test-project/_apis/wit/workitems/123?api-version=7.1",
            )
            .match_header("content-type", "application/json-patch+json")
            .match_body(mockito::Matcher::Json(json!([
                {
                    "op": "add",
                    "path": "/fields/System.State",
                    "value": "Active"
                }
            ])))
            .with_status(200)
            .with_body(
                json!({
                    "id": 123,
                    "fields": {
                        "System.Title": "Title",
                        "System.WorkItemType": "User Story",
                        "System.State": "Active"
                    }
                })
                .to_string(),
            )
            .create_async()
            .await;

        let result = provider
            .update_work_item_state(&WorkItemId::from_int(123), "Active")
            .await
            .unwrap();
        assert_eq!(result.state, "Active");

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_update_pull_request() {
        let (mut server, provider) = setup_mock_server().await;

        let mock = server
            .mock(
                "PATCH",
                "/test-project/_apis/git/repositories/my-repo/pullrequests/123?api-version=7.1",
            )
            .match_body(mockito::Matcher::Json(json!({
                "title": "New Title",
                "isDraft": false,
                "status": "active"
            })))
            .with_status(200)
            .with_body(
                json!({
                    "pullRequestId": 123,
                    "title": "New Title",
                    "status": "active",
                    "sourceRefName": "refs/heads/feature",
                    "targetRefName": "refs/heads/main",
                    "isDraft": false
                })
                .to_string(),
            )
            .create_async()
            .await;

        let pr = provider
            .update_pull_request(
                "my-repo",
                "123",
                Some("New Title"),
                None,
                Some(false),
                Some("active"),
            )
            .await
            .unwrap();
        assert_eq!(pr.title, "New Title");

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_complete_pull_request() {
        let (mut server, provider) = setup_mock_server().await;

        let mock = server
            .mock(
                "PATCH",
                "/test-project/_apis/git/repositories/my-repo/pullrequests/123?api-version=7.1",
            )
            .match_body(mockito::Matcher::Json(json!({
                "status": "completed",
                "completionOptions": {
                    "mergeStrategy": "squash",
                    "deleteSourceBranch": true
                }
            })))
            .with_status(200)
            .create_async()
            .await;

        provider
            .complete_pull_request("my-repo", "123", MergeStrategy::Squash, true)
            .await
            .unwrap();

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_add_reviewer() {
        let (mut server, provider) = setup_mock_server().await;

        let mock = server.mock("PUT", "/test-project/_apis/git/repositories/my-repo/pullrequests/123/reviewers/user-id?api-version=7.1")
            .match_body(mockito::Matcher::Json(json!({
                "vote": 0,
                "hasDeclined": false,
                "isFlagged": false
            })))
            .with_status(200)
            .create_async().await;

        provider
            .add_reviewer("my-repo", "123", "user-id")
            .await
            .unwrap();

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_work_item() {
        let (mut server, provider) = setup_mock_server().await;

        let mock = server
            .mock("GET", "/test-project/_apis/wit/workitems/123")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("$expand".to_string(), "fields".to_string()),
                mockito::Matcher::UrlEncoded("api-version".to_string(), "7.1".to_string()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                json!({
                    "id": 123,
                    "fields": {
                        "System.Title": "Test Title",
                        "System.WorkItemType": "User Story",
                        "System.State": "New",
                        "System.Description": "Some description",
                        "System.Tags": "tag1; tag2",
                        "System.AssignedTo": { "displayName": "Jules" }
                    }
                })
                .to_string(),
            )
            .create_async()
            .await;

        let result = provider
            .get_work_item(&WorkItemId::from_int(123))
            .await
            .unwrap();
        assert_eq!(result.id, WorkItemId::from_int(123));
        assert_eq!(result.title, "Test Title");
        assert_eq!(result.work_item_type, "User Story");
        assert_eq!(result.state, "New");
        assert_eq!(result.description.unwrap(), "Some description");
        assert_eq!(result.tags, vec!["tag1", "tag2"]);
        assert_eq!(result.assigned_to.unwrap(), "Jules");

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_create_work_item() {
        let (mut server, provider) = setup_mock_server().await;

        let mock = server
            .mock(
                "POST",
                "/test-project/_apis/wit/workitems/$User%20Story?api-version=7.1",
            )
            .match_header("content-type", "application/json-patch+json")
            .match_body(mockito::Matcher::Json(json!([
                {
                    "op": "add",
                    "path": "/fields/System.Title",
                    "value": "New Task"
                },
                {
                    "op": "add",
                    "path": "/fields/System.Description",
                    "value": "Desc"
                },
                {
                    "op": "add",
                    "path": "/fields/System.AssignedTo",
                    "value": "user@test.com"
                },
                {
                    "op": "add",
                    "path": "/fields/System.Tags",
                    "value": "tag1;tag2"
                }
            ])))
            .with_status(201)
            .with_body(
                json!({
                    "id": 456,
                    "fields": {
                        "System.Title": "New Task",
                        "System.WorkItemType": "User Story",
                        "System.State": "New"
                    }
                })
                .to_string(),
            )
            .create_async()
            .await;

        let result = provider
            .create_work_item(
                "New Task",
                "User Story",
                Some("Desc"),
                Some("user@test.com"),
                Some(vec!["tag1", "tag2"]),
            )
            .await
            .unwrap();
        assert_eq!(result.id, WorkItemId::from_int(456));
        assert_eq!(result.title, "New Task");

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_query_work_items() {
        let (mut server, provider) = setup_mock_server().await;

        let mock_wiql = server
            .mock("POST", "/test-project/_apis/wit/wiql?api-version=7.1")
            .with_status(200)
            .with_body(
                json!({
                    "workItems": [
                        { "id": 1 },
                        { "id": 2 }
                    ]
                })
                .to_string(),
            )
            .create_async()
            .await;

        let mock_batch = server
            .mock("POST", "/test-project/_apis/wit/workitemsbatch?api-version=7.1")
            .match_body(mockito::Matcher::Json(json!({
                "ids": ["1", "2"],
                "fields": [
                    "System.Id",
                    "System.Title",
                    "System.WorkItemType",
                    "System.State",
                    "System.Description",
                    "Microsoft.VSTS.TCM.ReproductionSteps",
                    "System.AssignedTo",
                    "System.Tags"
                ]
            })))
            .with_status(200)
            .with_body(
                json!({
                    "value": [
                        { "id": 1, "fields": { "System.Title": "T1", "System.WorkItemType": "Task", "System.State": "New" } },
                        { "id": 2, "fields": { "System.Title": "T2", "System.WorkItemType": "Task", "System.State": "New" } }
                    ]
                })
                .to_string(),
            )
            .create_async()
            .await;

        let filter = WorkItemFilter {
            state: Some("New".to_string()),
            ..Default::default()
        };
        let result = provider.query_work_items(&filter).await.unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, WorkItemId::from_int(1));
        assert_eq!(result[1].id, WorkItemId::from_int(2));

        mock_wiql.assert_async().await;
        mock_batch.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_child_work_items() {
        let (mut server, provider) = setup_mock_server().await;

        let mock_get_rels = server
            .mock("GET", "/test-project/_apis/wit/workitems/123")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("$expand".to_string(), "relations".to_string()),
                mockito::Matcher::UrlEncoded("api-version".to_string(), "7.1".to_string()),
            ]))
            .with_status(200)
            .with_body(
                json!({
                    "id": 123,
                    "relations": [
                        {
                            "rel": "System.LinkTypes.Hierarchy-Forward",
                            "url": "https://dev.azure.com/org/proj/_apis/wit/workItems/456"
                        },
                        {
                            "rel": "System.LinkTypes.Hierarchy-Forward",
                            "url": "https://dev.azure.com/org/proj/_apis/wit/workItems/789"
                        },
                        {
                            "rel": "System.LinkTypes.Hierarchy-Reverse",
                            "url": "https://dev.azure.com/org/proj/_apis/wit/workItems/111"
                        }
                    ]
                })
                .to_string(),
            )
            .create_async()
            .await;

        let mock_batch = server
            .mock("POST", "/test-project/_apis/wit/workitemsbatch?api-version=7.1")
            .match_body(mockito::Matcher::Json(json!({
                "ids": ["456", "789"],
                "fields": [
                    "System.Id",
                    "System.Title",
                    "System.WorkItemType",
                    "System.State",
                    "System.Description",
                    "Microsoft.VSTS.TCM.ReproductionSteps",
                    "System.AssignedTo",
                    "System.Tags"
                ]
            })))
            .with_status(200)
            .with_body(
                json!({
                    "value": [
                        { "id": 456, "fields": { "System.Title": "C1", "System.WorkItemType": "Task", "System.State": "New" } },
                        { "id": 789, "fields": { "System.Title": "C2", "System.WorkItemType": "Task", "System.State": "New" } }
                    ]
                })
                .to_string(),
            )
            .create_async()
            .await;

        let result = provider
            .get_child_work_items(&WorkItemId::from_int(123), None)
            .await
            .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, WorkItemId::from_int(456));
        assert_eq!(result[1].id, WorkItemId::from_int(789));

        mock_get_rels.assert_async().await;
        mock_batch.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_pull_request_details() {
        let (mut server, provider) = setup_mock_server().await;

        let mock = server
            .mock(
                "GET",
                "/test-project/_apis/git/repositories/my-repo/pullrequests/789?api-version=7.1",
            )
            .with_status(200)
            .with_body(
                json!({
                    "pullRequestId": 789,
                    "title": "PR Title",
                    "status": "active",
                    "sourceRefName": "refs/heads/feature",
                    "targetRefName": "refs/heads/main",
                    "isDraft": false
                })
                .to_string(),
            )
            .create_async()
            .await;

        let result = provider
            .get_pull_request_details("my-repo", "789")
            .await
            .unwrap();
        assert_eq!(result.id, "789");
        assert_eq!(result.title, "PR Title");

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_pull_request_by_branch() {
        let (mut server, provider) = setup_mock_server().await;

        let mock = server.mock("GET", "/test-project/_apis/git/repositories/my-repo/pullrequests?searchCriteria.sourceRefName=refs/heads/feature&api-version=7.1")
            .with_status(200)
            .with_body(json!({
                "value": [{
                    "pullRequestId": 123,
                    "title": "PR 1",
                    "status": "active",
                    "sourceRefName": "refs/heads/feature",
                    "targetRefName": "refs/heads/main",
                    "isDraft": false
                }]
            }).to_string())
            .create_async().await;

        let pr = provider
            .get_pull_request_by_branch("my-repo", "feature")
            .await
            .unwrap();
        assert!(pr.is_some());
        assert_eq!(pr.unwrap().id, "123");

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

        let mock_create_ref = server
            .mock(
                "POST",
                "/test-project/_apis/git/repositories/my-repo/refs?api-version=7.1",
            )
            .match_body(mockito::Matcher::Json(json!([
                {
                    "name": "refs/heads/new-branch",
                    "oldObjectId": "0000000000000000000000000000000000000000",
                    "newObjectId": "sha123"
                }
            ])))
            .with_status(201)
            .create_async()
            .await;

        provider
            .create_branch("my-repo", "new-branch", "main")
            .await
            .unwrap();

        mock_get_ref.assert_async().await;
        mock_create_ref.assert_async().await;
    }

    #[tokio::test]
    async fn test_list_pipelines() {
        let (mut server, provider) = setup_mock_server().await;

        let mock = server
            .mock("GET", "/test-project/_apis/pipelines?api-version=7.1")
            .with_status(200)
            .with_body(
                json!({
                    "value": [
                        { "id": 1, "name": "Pipe 1", "folder": "\\" },
                        { "id": 2, "name": "Pipe 2", "folder": "\\nested" }
                    ]
                })
                .to_string(),
            )
            .create_async()
            .await;

        let result = provider.list_pipelines().await.unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "Pipe 1");

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_latest_run() {
        let (mut server, provider) = setup_mock_server().await;

        let mock = server
            .mock(
                "GET",
                "/test-project/_apis/pipelines/runs?branchName=refs/heads/feature&api-version=7.1",
            )
            .with_status(200)
            .with_body(
                json!({
                    "value": [
                        {
                            "id": 100,
                            "state": "completed",
                            "result": "succeeded",
                            "_links": {
                                "web": { "href": "http://run-url" }
                            }
                        }
                    ]
                })
                .to_string(),
            )
            .create_async()
            .await;

        let run = provider.get_latest_run("feature").await.unwrap().unwrap();
        assert_eq!(run.id, "100");
        assert_eq!(run.status, "completed");
        assert_eq!(run.result.unwrap(), "succeeded");

        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_run_status() {
        let (mut server, provider) = setup_mock_server().await;

        let mock = server
            .mock(
                "GET",
                "/test-project/_apis/pipelines/runs/123?api-version=7.1",
            )
            .with_status(200)
            .with_body(
                json!({
                    "id": 123,
                    "state": "inProgress",
                    "_links": {
                        "web": { "href": "http://run-url" }
                    }
                })
                .to_string(),
            )
            .create_async()
            .await;

        let run = provider.get_run_status("123").await.unwrap();
        assert_eq!(run.id, "123");
        assert_eq!(run.status, "inProgress");
        assert!(run.result.is_none());

        mock.assert_async().await;
    }
}

#[async_trait]
impl VCSProvider for AzureDevOpsProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            draft_pull_requests: true,
            pipeline_support: true,
            merge_strategies: vec![
                MergeStrategy::Squash,
                MergeStrategy::Rebase,
                MergeStrategy::RebaseMerge,
                MergeStrategy::NoFastForward,
            ],
            ..Default::default()
        }
    }

    async fn get_pull_request_by_branch(
        &self,
        repository: &str,
        branch: &str,
    ) -> Result<Option<PullRequest>> {
        let url = self.v(&format!(
            "{}/git/repositories/{}/pullrequests?searchCriteria.sourceRefName={}",
            self.base_api_url(),
            repository,
            self.normalize_ref(branch)
        ));
        let resp = self.client.get(url).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!("Failed to list PRs: {}", resp.text().await?));
        }

        let body: Value = resp.json().await?;
        let items = body["value"]
            .as_array()
            .ok_or_else(|| anyhow!("No value array in response"))?;

        if items.is_empty() {
            return Ok(None);
        }

        let item = &items[0];
        Ok(Some(PullRequest {
            id: item["pullRequestId"]
                .as_i64()
                .unwrap_or_default()
                .to_string(),
            title: item["title"].as_str().unwrap_or_default().to_string(),
            status: item["status"].as_str().unwrap_or_default().to_string(),
            source_branch: item["sourceRefName"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            target_branch: item["targetRefName"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            is_draft: item["isDraft"].as_bool().unwrap_or_default(),
        }))
    }

    async fn get_pull_request_details(&self, repository: &str, id: &str) -> Result<PullRequest> {
        let url = self.v(&format!(
            "{}/git/repositories/{}/pullrequests/{}",
            self.base_api_url(),
            repository,
            id
        ));
        let resp = self.client.get(url).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!("Failed to get PR {}: {}", id, resp.text().await?));
        }

        let body: Value = resp.json().await?;

        Ok(PullRequest {
            id: id.to_string(),
            title: body["title"].as_str().unwrap_or_default().to_string(),
            status: body["status"].as_str().unwrap_or_default().to_string(),
            source_branch: body["sourceRefName"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            target_branch: body["targetRefName"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            is_draft: body["isDraft"].as_bool().unwrap_or_default(),
        })
    }

    async fn create_pull_request(
        &self,
        repository: &str,
        source: &str,
        target: &str,
        title: &str,
        description: &str,
        is_draft: bool,
        work_item_refs: &[&WorkItemId],
    ) -> Result<PullRequest> {
        let url = self.v(&format!(
            "{}/git/repositories/{}/pullrequests",
            self.base_api_url(),
            repository
        ));

        let wi_refs: Vec<serde_json::Value> = work_item_refs
            .iter()
            .map(|id| json!({ "id": id.to_string() }))
            .collect();

        let body = json!({
            "sourceRefName": self.normalize_ref(source),
            "targetRefName": self.normalize_ref(target),
            "title": title,
            "description": description,
            "isDraft": is_draft,
            "workItemRefs": wi_refs
        });

        let resp = self.client.post(url).json(&body).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!("Failed to create PR: {}", resp.text().await?));
        }

        let body: Value = resp.json().await?;
        let id = body["pullRequestId"]
            .as_i64()
            .ok_or_else(|| anyhow!("No pullRequestId in response"))?
            .to_string();

        Ok(PullRequest {
            id,
            title: body["title"].as_str().unwrap_or_default().to_string(),
            status: body["status"].as_str().unwrap_or_default().to_string(),
            source_branch: body["sourceRefName"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            target_branch: body["targetRefName"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            is_draft: body["isDraft"].as_bool().unwrap_or_default(),
        })
    }

    async fn create_branch(&self, repository: &str, name: &str, source: &str) -> Result<()> {
        let normalized_source = self.normalize_ref(source);
        let normalized_name = self.normalize_ref(name);

        // Get source branch objectId
        let refs_url = self.v(&format!(
            "{}/git/repositories/{}/refs?filter={}",
            self.base_api_url(),
            repository,
            normalized_source.trim_start_matches("refs/")
        ));
        let resp = self.client.get(refs_url).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "Failed to find source branch {}: {}",
                source,
                resp.text().await?
            ));
        }

        let body: Value = resp.json().await?;
        let source_sha = body["value"][0]["objectId"]
            .as_str()
            .ok_or_else(|| anyhow!("Could not find objectId for source branch {}", source))?;

        let create_url = self.v(&format!(
            "{}/git/repositories/{}/refs",
            self.base_api_url(),
            repository
        ));
        let body = json!([
            {
                "name": normalized_name,
                "oldObjectId": "0000000000000000000000000000000000000000",
                "newObjectId": source_sha
            }
        ]);

        let resp = self.client.post(create_url).json(&body).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "Failed to create branch {}: {}",
                name,
                resp.text().await?
            ));
        }

        Ok(())
    }

    async fn get_repository(&self, name: &str) -> Result<Repository> {
        let url = self.v(&format!(
            "{}/git/repositories/{}",
            self.base_api_url(),
            name
        ));
        let resp = self.client.get(url).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "Failed to get repository {}: {}",
                name,
                resp.text().await?
            ));
        }

        let body: Value = resp.json().await?;

        Ok(Repository {
            id: body["id"].as_str().unwrap_or_default().to_string(),
            name: body["name"].as_str().unwrap_or_default().to_string(),
            project_id: body["project"]["id"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            default_branch: body["defaultBranch"].as_str().map(|s| s.to_string()),
        })
    }

    async fn delete_branch(&self, repository: &str, name: &str) -> Result<()> {
        let normalized_name = self.normalize_ref(name);

        // Get current branch objectId
        let refs_url = self.v(&format!(
            "{}/git/repositories/{}/refs?filter={}",
            self.base_api_url(),
            repository,
            normalized_name.trim_start_matches("refs/")
        ));
        let resp = self.client.get(refs_url).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "Failed to find branch {}: {}",
                name,
                resp.text().await?
            ));
        }

        let body: Value = resp.json().await?;
        let current_sha = body["value"][0]["objectId"]
            .as_str()
            .ok_or_else(|| anyhow!("Could not find objectId for branch {}", name))?;

        let update_url = self.v(&format!(
            "{}/git/repositories/{}/refs",
            self.base_api_url(),
            repository
        ));
        let body = json!([
            {
                "name": normalized_name,
                "oldObjectId": current_sha,
                "newObjectId": "0000000000000000000000000000000000000000"
            }
        ]);

        let resp = self.client.post(update_url).json(&body).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "Failed to delete branch {}: {}",
                name,
                resp.text().await?
            ));
        }

        Ok(())
    }

    async fn update_pull_request(
        &self,
        repository: &str,
        id: &str,
        title: Option<&str>,
        description: Option<&str>,
        is_draft: Option<bool>,
        status: Option<&str>,
    ) -> Result<PullRequest> {
        let url = self.v(&format!(
            "{}/git/repositories/{}/pullrequests/{}",
            self.base_api_url(),
            repository,
            id
        ));

        let mut body = json!({});
        if let Some(t) = title {
            body["title"] = json!(t);
        }
        if let Some(d) = description {
            body["description"] = json!(d);
        }
        if let Some(draft) = is_draft {
            body["isDraft"] = json!(draft);
        }
        if let Some(s) = status {
            body["status"] = json!(s);
        }

        let resp = self.client.patch(url).json(&body).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "Failed to update PR {}: {}",
                id,
                resp.text().await?
            ));
        }

        let body: Value = resp.json().await?;

        Ok(PullRequest {
            id: id.to_string(),
            title: body["title"].as_str().unwrap_or_default().to_string(),
            status: body["status"].as_str().unwrap_or_default().to_string(),
            source_branch: body["sourceRefName"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            target_branch: body["targetRefName"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            is_draft: body["isDraft"].as_bool().unwrap_or_default(),
        })
    }

    async fn complete_pull_request(
        &self,
        repository: &str,
        id: &str,
        strategy: MergeStrategy,
        delete_source_branch: bool,
    ) -> Result<()> {
        let url = self.v(&format!(
            "{}/git/repositories/{}/pullrequests/{}",
            self.base_api_url(),
            repository,
            id
        ));

        let body = json!({
            "status": "completed",
            "completionOptions": {
                "mergeStrategy": strategy,
                "deleteSourceBranch": delete_source_branch
            }
        });

        let resp = self.client.patch(url).json(&body).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "Failed to complete PR {}: {}",
                id,
                resp.text().await?
            ));
        }

        Ok(())
    }

    async fn add_reviewer(&self, repository: &str, id: &str, reviewer_id: &str) -> Result<()> {
        let url = self.v(&format!(
            "{}/git/repositories/{}/pullrequests/{}/reviewers/{}",
            self.base_api_url(),
            repository,
            id,
            reviewer_id
        ));

        let body = json!({
            "vote": 0,
            "hasDeclined": false,
            "isFlagged": false
        });

        let resp = self.client.put(url).json(&body).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "Failed to add reviewer {} to PR {}: {}",
                reviewer_id,
                id,
                resp.text().await?
            ));
        }

        Ok(())
    }

    async fn get_current_branch(&self) -> Result<String> {
        Err(anyhow!(
            "Not implemented for Azure DevOps provider (use LocalGitProvider)"
        ))
    }
    async fn checkout_branch(&self, _name: &str) -> Result<()> {
        Err(anyhow!(
            "Not implemented for Azure DevOps provider (use LocalGitProvider)"
        ))
    }
    async fn get_status(&self) -> Result<String> {
        Err(anyhow!(
            "Not implemented for Azure DevOps provider (use LocalGitProvider)"
        ))
    }
    async fn stash_push(&self, _message: &str) -> Result<()> {
        Err(anyhow!(
            "Not implemented for Azure DevOps provider (use LocalGitProvider)"
        ))
    }
    async fn stash_pop(&self) -> Result<()> {
        Err(anyhow!(
            "Not implemented for Azure DevOps provider (use LocalGitProvider)"
        ))
    }
    async fn push(&self, _force: bool) -> Result<()> {
        Err(anyhow!(
            "Not implemented for Azure DevOps provider (use LocalGitProvider)"
        ))
    }
    async fn pull(&self) -> Result<()> {
        Err(anyhow!(
            "Not implemented for Azure DevOps provider (use LocalGitProvider)"
        ))
    }
    async fn fetch(&self) -> Result<()> {
        Err(anyhow!(
            "Not implemented for Azure DevOps provider (use LocalGitProvider)"
        ))
    }
    async fn commit(&self, _message: &str, _all: bool, _amend: bool) -> Result<()> {
        Err(anyhow!(
            "Not implemented for Azure DevOps provider (use LocalGitProvider)"
        ))
    }
    async fn discard_local_changes(&self) -> Result<()> {
        Err(anyhow!(
            "Not implemented for Azure DevOps provider (use LocalGitProvider)"
        ))
    }
    async fn get_log(&self, _range: Option<&str>, _limit: Option<i32>) -> Result<String> {
        Err(anyhow!(
            "Not implemented for Azure DevOps provider (use LocalGitProvider)"
        ))
    }
    async fn merge(&self, _source: &str) -> Result<()> {
        Err(anyhow!(
            "Not implemented for Azure DevOps provider (use LocalGitProvider)"
        ))
    }
    async fn rebase(&self, _target: &str) -> Result<()> {
        Err(anyhow!(
            "Not implemented for Azure DevOps provider (use LocalGitProvider)"
        ))
    }
    async fn check_submodule_status(&self, _path: &str) -> Result<bool> {
        Err(anyhow!(
            "Not implemented for Azure DevOps provider (use LocalGitProvider)"
        ))
    }
    async fn update_submodule_pointer(&self, _path: &str) -> Result<()> {
        Err(anyhow!(
            "Not implemented for Azure DevOps provider (use LocalGitProvider)"
        ))
    }
}

#[async_trait]
impl PipelineProvider for AzureDevOpsProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            pipeline_support: true,
            ..Default::default()
        }
    }

    async fn list_pipelines(&self) -> Result<Vec<Pipeline>> {
        let url = self.v(&format!("{}/pipelines", self.base_api_url()));
        let resp = self.client.get(url).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!("Failed to list pipelines: {}", resp.text().await?));
        }

        let body: Value = resp.json().await?;
        let items = body["value"]
            .as_array()
            .ok_or_else(|| anyhow!("No value array in response"))?;

        let mut pipelines = Vec::new();
        for item in items {
            pipelines.push(Pipeline {
                id: item["id"].as_i64().unwrap_or_default().to_string(),
                name: item["name"].as_str().unwrap_or_default().to_string(),
                folder: item["folder"].as_str().unwrap_or_default().to_string(),
            });
        }

        Ok(pipelines)
    }

    async fn run_pipeline(&self, pipeline_id: &str, branch: &str) -> Result<PipelineRun> {
        let url = self.v(&format!(
            "{}/pipelines/{}/runs",
            self.base_api_url(),
            pipeline_id
        ));

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
            return Err(anyhow!(
                "Failed to run pipeline {}: {}",
                pipeline_id,
                resp.text().await?
            ));
        }

        let body: Value = resp.json().await?;

        Ok(PipelineRun {
            id: body["id"].as_i64().unwrap_or_default().to_string(),
            status: body["state"].as_str().unwrap_or_default().to_string(),
            result: body["result"].as_str().map(|s| s.to_string()),
            url: body["_links"]["web"]["href"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        })
    }

    async fn get_latest_run(&self, branch: &str) -> Result<Option<PipelineRun>> {
        let url = self.v(&format!(
            "{}/pipelines/runs?branchName={}",
            self.base_api_url(),
            self.normalize_ref(branch)
        ));
        let resp = self.client.get(url).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!("Failed to list runs: {}", resp.text().await?));
        }

        let body: Value = resp.json().await?;
        let items = body["value"]
            .as_array()
            .ok_or_else(|| anyhow!("No value array in response"))?;

        if items.is_empty() {
            return Ok(None);
        }

        let body = &items[0];

        Ok(Some(PipelineRun {
            id: body["id"].as_i64().unwrap_or_default().to_string(),
            status: body["state"].as_str().unwrap_or_default().to_string(),
            result: body["result"].as_str().map(|s| s.to_string()),
            url: body["_links"]["web"]["href"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        }))
    }

    async fn get_run_status(&self, run_id: &str) -> Result<PipelineRun> {
        let url = self.v(&format!(
            "{}/pipelines/runs/{}",
            self.base_api_url(),
            run_id
        ));
        let resp = self.client.get(url).send().await?;

        if !resp.status().is_success() {
            return Err(anyhow!(
                "Failed to get pipeline run {}: {}",
                run_id,
                resp.text().await?
            ));
        }

        let body: Value = resp.json().await?;

        Ok(PipelineRun {
            id: body["id"].as_i64().unwrap_or_default().to_string(),
            status: body["state"].as_str().unwrap_or_default().to_string(),
            result: body["result"].as_str().map(|s| s.to_string()),
            url: body["_links"]["web"]["href"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        })
    }
}
