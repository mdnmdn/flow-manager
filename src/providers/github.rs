use crate::core::config::GitHubConfig;
use crate::core::models::{
    ChangedFile, MergeStrategy, Pipeline, PipelineRun, ProviderCapabilities, PullRequest,
    PullRequestComment, PullRequestThread, Repository, WorkItem, WorkItemComment, WorkItemFilter,
    WorkItemId,
};
use crate::providers::{IssueTracker, PipelineProvider, VCSProvider};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::{header, Client};
use serde_json::{json, Value};

pub struct GitHubProvider {
    client: Client,
    owner: String,
    repo: String,
    base_url: String,
}

fn resolve_token(config: &GitHubConfig) -> Result<String> {
    // 1. Explicit PAT from fm.toml / FM__PROVIDER__GITHUB__TOKEN env var
    if let Some(t) = &config.token {
        if !t.is_empty() {
            return Ok(t.clone());
        }
    }

    // 2. GITHUB_TOKEN — set automatically in GitHub Actions and accepted by the gh CLI
    if let Ok(t) = std::env::var("GITHUB_TOKEN") {
        if !t.is_empty() {
            return Ok(t);
        }
    }

    // 3. GH_TOKEN — alternate env var used by the gh CLI
    if let Ok(t) = std::env::var("GH_TOKEN") {
        if !t.is_empty() {
            return Ok(t);
        }
    }

    // 4. OS keychain (GitHub App Device Flow), keyed by the configured account alias
    let account = &config.account;
    match crate::auth::token_store::load(account) {
        Ok(Some(stored)) => {
            if stored.is_expired() {
                return Err(anyhow!(
                    "GitHub App token for account '{}' expired.\n\
                     Run `fm auth login --account {}` to re-authenticate.",
                    account,
                    account
                ));
            }
            Ok(stored.access_token)
        }
        Ok(None) => Err(anyhow!(
            "No GitHub token found. Tried (in order):\n\
             - `token` field in [provider.github] / FM__PROVIDER__GITHUB__TOKEN env\n\
             - GITHUB_TOKEN env var (auto-set in GitHub Actions)\n\
             - GH_TOKEN env var\n\
             - OS keychain account '{}'\n\
             \n\
             To fix: set one of the above, or run `fm auth login` to authenticate via GitHub App.",
            account
        )),
        Err(e) => Err(anyhow!("Failed to load GitHub App token: {}", e)),
    }
}

impl GitHubProvider {
    pub fn new(config: &GitHubConfig) -> Result<Self> {
        let token = resolve_token(config)?;

        let mut headers = header::HeaderMap::new();
        let mut auth_value = header::HeaderValue::from_str(&format!("Bearer {}", token))?;
        auth_value.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, auth_value);
        headers.insert(
            header::ACCEPT,
            header::HeaderValue::from_static("application/vnd.github.v3+json"),
        );
        headers.insert(
            header::USER_AGENT,
            header::HeaderValue::from_static("flow-manager"),
        );

        let client = Client::builder().default_headers(headers).build()?;

        let base_url = config
            .base_url
            .as_ref()
            .map(|u| u.trim_end_matches('/').to_string())
            .unwrap_or_else(|| "https://api.github.com".to_string());

        Ok(Self {
            client,
            owner: config.owner.clone(),
            repo: config.repo.clone(),
            base_url,
        })
    }

    fn repo_url(&self, path: &str) -> String {
        format!(
            "{}/repos/{}/{}{}",
            self.base_url, self.owner, self.repo, path
        )
    }

    fn web_base_url(&self) -> String {
        if self.base_url.contains("api.github") {
            "https://github.com".to_string()
        } else {
            self.base_url
                .strip_suffix("/api")
                .unwrap_or(&self.base_url)
                .trim_end_matches('/')
                .to_string()
        }
    }

    fn type_to_label(work_item_type: &str) -> String {
        match work_item_type {
            "Bug" => "type:bug".to_string(),
            "Feature" => "type:feature".to_string(),
            "Task" => "type:task".to_string(),
            "User Story" => "type:user-story".to_string(),
            other => format!("type:{}", other.to_lowercase()),
        }
    }

    fn parse_issue(&self, body: &Value) -> Result<WorkItem> {
        let id = body["number"]
            .as_i64()
            .ok_or_else(|| anyhow!("No number in issue: {:?}", body))?;

        let labels = body["labels"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|label| label["name"].as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Ok(WorkItem {
            id: WorkItemId::from_int(id as i32),
            title: body["title"].as_str().unwrap_or_default().to_string(),
            work_item_type: Self::extract_type_from_labels(&labels),
            state: body["state"].as_str().unwrap_or_default().to_string(),
            description: body["body"].as_str().map(|s| s.to_string()),
            assigned_to: body["assignee"]["login"].as_str().map(|s| s.to_string()),
            tags: labels,
            comments_count: body["comments"].as_i64().map(|c| c as i32),
        })
    }

    fn extract_type_from_labels(labels: &[String]) -> String {
        labels
            .iter()
            .find(|l| l.starts_with("type:"))
            .map(|l| {
                let stripped = l.strip_prefix("type:").unwrap_or("");
                match stripped {
                    "bug" => "Bug",
                    "feature" => "Feature",
                    "task" => "Task",
                    "user-story" => "User Story",
                    _ => "Task",
                }
                .to_string()
            })
            .unwrap_or_else(|| "Task".to_string())
    }

    fn parse_pull_request(&self, body: &Value) -> Result<PullRequest> {
        Ok(PullRequest {
            id: body["number"]
                .as_i64()
                .map(|n| n.to_string())
                .ok_or_else(|| anyhow!("No PR number"))?,
            title: body["title"].as_str().unwrap_or_default().to_string(),
            status: body["state"].as_str().unwrap_or("open").to_string(),
            source_branch: body["head"]["ref"].as_str().unwrap_or_default().to_string(),
            target_branch: body["base"]["ref"].as_str().unwrap_or_default().to_string(),
            is_draft: body["draft"].as_bool().unwrap_or(false),
            description: body["body"].as_str().map(|s| s.to_string()),
            author: body["user"]["login"].as_str().map(|s| s.to_string()),
            created_at: body["created_at"].as_str().map(|s| s.to_string()),
        })
    }

    fn build_search_query(&self, filter: &WorkItemFilter) -> String {
        let mut terms = vec![
            format!("repo:{}/{}", self.owner, self.repo),
            "is:issue".to_string(),
        ];

        if let Some(state) = &filter.state {
            if state == "closed" {
                terms.push("is:closed".to_string());
            } else {
                terms.push("is:open".to_string());
            }
        }

        if let Some(assigned_to) = &filter.assigned_to {
            terms.push(format!("assignee:{}", assigned_to));
        }

        for label in &filter.labels {
            terms.push(format!("label:{}", label));
        }

        if let Some(wi_type) = &filter.work_item_type {
            let type_label = Self::type_to_label(wi_type);
            terms.push(format!("label:{}", type_label));
        }

        if let Some(text) = &filter.text {
            terms.push(format!("\"{}\"", text));
        }

        if let Some(milestone) = &filter.milestone {
            terms.push(format!("milestone:{}", milestone));
        }

        terms.join(" ")
    }

    async fn get_authenticated_login(&self) -> Option<String> {
        let url = format!("{}/user", self.base_url);
        let resp = self.client.get(&url).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        let body: Value = resp.json().await.ok()?;
        body["login"].as_str().map(|s| s.to_string())
    }
}

#[async_trait]
impl IssueTracker for GitHubProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            draft_pull_requests: true,
            pipeline_support: true,
            work_item_hierarchy: false,
            formal_artifact_links: false,
            merge_strategies: vec![
                MergeStrategy::Squash,
                MergeStrategy::Rebase,
                MergeStrategy::RebaseMerge,
                MergeStrategy::NoFastForward,
            ],
            work_item_relations: vec!["relates_to".to_string(), "blocks".to_string()],
        }
    }

    async fn get_work_item(&self, id: &WorkItemId) -> Result<WorkItem> {
        let url = self.repo_url(&format!("/issues/{}", id.as_str()));
        let resp = self.client.get(&url).send().await?;
        resp.error_for_status_ref()?;
        let body: Value = resp.json().await?;
        self.parse_issue(&body)
    }

    async fn create_work_item(
        &self,
        title: &str,
        work_item_type: &str,
        description: Option<&str>,
        assigned_to: Option<&str>,
        tags: Option<Vec<&str>>,
    ) -> Result<WorkItem> {
        let mut labels = vec![Self::type_to_label(work_item_type)];
        if let Some(tag_vec) = tags {
            labels.extend(tag_vec.iter().map(|t| t.to_string()));
        }

        let mut payload = json!({
            "title": title,
            "labels": labels,
        });

        if let Some(desc) = description {
            payload["body"] = json!(desc);
        }

        let resolved_assignee = if let Some(assigned) = assigned_to {
            Some(assigned.to_string())
        } else {
            self.get_authenticated_login().await
        };

        if let Some(assigned) = resolved_assignee {
            payload["assignees"] = json!(vec![assigned]);
        }

        let url = self.repo_url("/issues");
        let resp = self.client.post(&url).json(&payload).send().await?;
        resp.error_for_status_ref()?;
        let body: Value = resp.json().await?;
        self.parse_issue(&body)
    }

    async fn update_work_item(
        &self,
        id: &WorkItemId,
        title: Option<&str>,
        description: Option<&str>,
        assigned_to: Option<&str>,
        tags: Option<Vec<&str>>,
    ) -> Result<WorkItem> {
        let mut payload = json!({});

        if let Some(t) = title {
            payload["title"] = json!(t);
        }
        if let Some(d) = description {
            payload["body"] = json!(d);
        }
        if let Some(a) = assigned_to {
            payload["assignees"] = json!(vec![a]);
        }
        if let Some(t) = tags {
            payload["labels"] = json!(t);
        }

        let url = self.repo_url(&format!("/issues/{}", id.as_str()));
        let resp = self.client.patch(&url).json(&payload).send().await?;
        resp.error_for_status_ref()?;
        let body: Value = resp.json().await?;
        self.parse_issue(&body)
    }

    async fn update_work_item_state(&self, id: &WorkItemId, state: &str) -> Result<WorkItem> {
        let gh_state = match state {
            "open" | "active" => "open",
            _ => "closed",
        };

        let payload = json!({ "state": gh_state });
        let url = self.repo_url(&format!("/issues/{}", id.as_str()));
        let resp = self.client.patch(&url).json(&payload).send().await?;
        resp.error_for_status_ref()?;
        let body: Value = resp.json().await?;
        self.parse_issue(&body)
    }

    async fn query_work_items(&self, filter: &WorkItemFilter) -> Result<Vec<WorkItem>> {
        let query = self.build_search_query(filter);
        let url = format!("{}/search/issues", self.base_url);

        let resp = self.client.get(&url).query(&[("q", &query)]).send().await?;
        resp.error_for_status_ref()?;
        let body: Value = resp.json().await?;

        let items = body["items"]
            .as_array()
            .ok_or_else(|| anyhow!("No items"))?;
        let mut results: Vec<WorkItem> = Vec::new();

        for item in items {
            if let Ok(wi) = self.parse_issue(item) {
                results.push(wi);
            }
        }

        if let Some(limit) = filter.limit {
            results.truncate(limit as usize);
        }

        Ok(results)
    }

    async fn create_artifact_link(&self, wi_id: &WorkItemId, url: &str) -> Result<()> {
        let body = json!({
            "body": format!("Branch: {}", url)
        });

        let endpoint = self.repo_url(&format!("/issues/{}/comments", wi_id.as_str()));
        let resp = self.client.post(&endpoint).json(&body).send().await?;
        resp.error_for_status_ref()?;
        Ok(())
    }

    async fn link_work_items(
        &self,
        source_id: &WorkItemId,
        target_id: &WorkItemId,
        _relation: &str,
    ) -> Result<()> {
        let body = json!({
            "body": format!("Relates to #{}", target_id.as_str())
        });

        let endpoint = self.repo_url(&format!("/issues/{}/comments", source_id.as_str()));
        let resp = self.client.post(&endpoint).json(&body).send().await?;
        resp.error_for_status_ref()?;
        Ok(())
    }

    async fn get_child_work_items(
        &self,
        _id: &WorkItemId,
        _work_item_type: Option<&str>,
    ) -> Result<Vec<WorkItem>> {
        Ok(vec![])
    }

    async fn available_states(&self, _id: &WorkItemId) -> Result<Vec<String>> {
        Ok(vec!["open".to_string(), "closed".to_string()])
    }

    async fn get_work_item_comments(&self, id: &WorkItemId) -> Result<Vec<WorkItemComment>> {
        let url = self.repo_url(&format!("/issues/{}/comments", id.as_str()));
        let resp = self.client.get(&url).send().await?;
        resp.error_for_status_ref()?;
        let body: Value = resp.json().await?;

        let comments = body.as_array().ok_or_else(|| anyhow!("Not an array"))?;
        let mut results = Vec::new();

        for c in comments {
            results.push(WorkItemComment {
                id: c["id"].to_string(),
                author: c["user"]["login"].as_str().unwrap_or_default().to_string(),
                created_at: c["created_at"].as_str().unwrap_or_default().to_string(),
                text: c["body"].as_str().unwrap_or_default().to_string(),
                created_at_date: None,
                created_at_time: None,
            });
        }

        Ok(results)
    }

    async fn add_work_item_comment(
        &self,
        id: &WorkItemId,
        comment: &str,
    ) -> Result<WorkItemComment> {
        let body = json!({ "body": comment });
        let url = self.repo_url(&format!("/issues/{}/comments", id.as_str()));
        let resp = self.client.post(&url).json(&body).send().await?;
        resp.error_for_status_ref()?;
        let response_body: Value = resp.json().await?;

        Ok(WorkItemComment {
            id: response_body["id"].to_string(),
            author: response_body["user"]["login"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            created_at: response_body["created_at"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            text: response_body["body"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            created_at_date: None,
            created_at_time: None,
        })
    }

    fn work_item_url(&self, id: &str) -> Result<String> {
        let base = self.web_base_url();
        Ok(format!(
            "{}/{}/{}/issues/{}",
            base, self.owner, self.repo, id
        ))
    }
}

#[async_trait]
impl VCSProvider for GitHubProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            draft_pull_requests: true,
            pipeline_support: true,
            work_item_hierarchy: false,
            formal_artifact_links: false,
            merge_strategies: vec![
                MergeStrategy::Squash,
                MergeStrategy::Rebase,
                MergeStrategy::RebaseMerge,
                MergeStrategy::NoFastForward,
            ],
            work_item_relations: vec!["relates_to".to_string(), "blocks".to_string()],
        }
    }

    async fn get_pull_request_by_branch(
        &self,
        _repository: &str,
        branch: &str,
    ) -> Result<Option<PullRequest>> {
        let url = self.repo_url(&format!("/pulls?head={}:{}&state=open", self.owner, branch));
        let resp = self.client.get(&url).send().await?;
        resp.error_for_status_ref()?;
        let body: Value = resp.json().await?;

        if let Some(arr) = body.as_array() {
            if let Some(first) = arr.first() {
                return Ok(Some(self.parse_pull_request(first)?));
            }
        }

        Ok(None)
    }

    async fn get_pull_request_details(&self, _repository: &str, id: &str) -> Result<PullRequest> {
        let url = self.repo_url(&format!("/pulls/{}", id));
        let resp = self.client.get(&url).send().await?;
        resp.error_for_status_ref()?;
        let body: Value = resp.json().await?;
        self.parse_pull_request(&body)
    }

    async fn create_pull_request(
        &self,
        _repository: &str,
        source: &str,
        target: &str,
        title: &str,
        description: &str,
        is_draft: bool,
        work_item_refs: &[&WorkItemId],
    ) -> Result<PullRequest> {
        let mut body_text = description.to_string();
        for wi_ref in work_item_refs {
            body_text.push_str(&format!("\n\nRelated: #{}", wi_ref.as_str()));
        }

        let payload = json!({
            "title": title,
            "body": body_text,
            "head": source,
            "base": target,
            "draft": is_draft,
        });

        let url = self.repo_url("/pulls");
        let resp = self.client.post(&url).json(&payload).send().await?;
        resp.error_for_status_ref()?;
        let body: Value = resp.json().await?;
        self.parse_pull_request(&body)
    }

    async fn update_pull_request(
        &self,
        _repository: &str,
        id: &str,
        title: Option<&str>,
        description: Option<&str>,
        is_draft: Option<bool>,
        status: Option<&str>,
    ) -> Result<PullRequest> {
        if let Some(s) = status {
            if s == "completed" || s == "abandoned" {
                return self
                    .complete_pull_request(_repository, id, MergeStrategy::Squash, false)
                    .await
                    .map(|_| PullRequest {
                        id: id.to_string(),
                        title: title.unwrap_or_default().to_string(),
                        status: "closed".to_string(),
                        source_branch: String::new(),
                        target_branch: String::new(),
                        is_draft: false,
                        description: None,
                        author: None,
                        created_at: None,
                    });
            }
        }

        let mut payload = json!({});
        if let Some(t) = title {
            payload["title"] = json!(t);
        }
        if let Some(d) = description {
            payload["body"] = json!(d);
        }
        if let Some(draft) = is_draft {
            payload["draft"] = json!(draft);
        }

        let url = self.repo_url(&format!("/pulls/{}", id));
        let resp = self.client.patch(&url).json(&payload).send().await?;
        resp.error_for_status_ref()?;
        let body: Value = resp.json().await?;
        self.parse_pull_request(&body)
    }

    async fn complete_pull_request(
        &self,
        _repository: &str,
        id: &str,
        strategy: MergeStrategy,
        delete_source_branch: bool,
    ) -> Result<()> {
        let merge_method = match strategy {
            MergeStrategy::Squash => "squash",
            MergeStrategy::Rebase => "rebase",
            MergeStrategy::RebaseMerge => "rebase",
            MergeStrategy::NoFastForward => "create",
        };

        let payload = json!({
            "merge_method": merge_method,
        });

        let url = self.repo_url(&format!("/pulls/{}/merge", id));
        let resp = self.client.put(&url).json(&payload).send().await?;
        resp.error_for_status_ref()?;

        if delete_source_branch {
            let pr_url = self.repo_url(&format!("/pulls/{}", id));
            let pr_resp = self.client.get(&pr_url).send().await?;
            pr_resp.error_for_status_ref()?;
            let pr_body: Value = pr_resp.json().await?;

            if let Some(source_branch) = pr_body["head"]["ref"].as_str() {
                let delete_url = self.repo_url(&format!("/git/refs/heads/{}", source_branch));
                let _ = self.client.delete(&delete_url).send().await;
            }
        }

        Ok(())
    }

    async fn add_reviewer(&self, _repository: &str, id: &str, reviewer_id: &str) -> Result<()> {
        let payload = json!({
            "reviewers": vec![reviewer_id],
        });

        let url = self.repo_url(&format!("/pulls/{}/requested_reviewers", id));
        let resp = self.client.post(&url).json(&payload).send().await?;
        resp.error_for_status_ref()?;
        Ok(())
    }

    async fn create_branch(&self, _repository: &str, name: &str, source: &str) -> Result<()> {
        let ref_url = self.repo_url(&format!("/git/ref/heads/{}", source));
        let ref_resp = self.client.get(&ref_url).send().await?;
        ref_resp.error_for_status_ref()?;
        let ref_body: Value = ref_resp.json().await?;

        let sha = ref_body["object"]["sha"]
            .as_str()
            .ok_or_else(|| anyhow!("No SHA found"))?;

        let payload = json!({
            "ref": format!("refs/heads/{}", name),
            "sha": sha,
        });

        let url = self.repo_url("/git/refs");
        let resp = self.client.post(&url).json(&payload).send().await?;
        resp.error_for_status_ref()?;
        Ok(())
    }

    async fn delete_branch(&self, _repository: &str, name: &str) -> Result<()> {
        let url = self.repo_url(&format!("/git/refs/heads/{}", name));
        let resp = self.client.delete(&url).send().await?;
        resp.error_for_status_ref()?;
        Ok(())
    }

    async fn get_repository(&self, _name: &str) -> Result<Repository> {
        let url = self.repo_url("");
        let resp = self.client.get(&url).send().await?;
        resp.error_for_status_ref()?;
        let body: Value = resp.json().await?;

        Ok(Repository {
            id: body["id"].to_string(),
            name: body["full_name"].as_str().unwrap_or_default().to_string(),
            project_id: body["owner"]["login"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            default_branch: body["default_branch"].as_str().map(|s| s.to_string()),
        })
    }

    async fn get_current_branch(&self) -> Result<String> {
        Err(anyhow!(
            "Not implemented for GitHub provider (use LocalGitProvider)"
        ))
    }

    async fn checkout_branch(&self, _name: &str) -> Result<()> {
        Err(anyhow!(
            "Not implemented for GitHub provider (use LocalGitProvider)"
        ))
    }

    async fn get_status(&self) -> Result<String> {
        Err(anyhow!(
            "Not implemented for GitHub provider (use LocalGitProvider)"
        ))
    }

    async fn stash_push(&self, _message: &str) -> Result<()> {
        Err(anyhow!(
            "Not implemented for GitHub provider (use LocalGitProvider)"
        ))
    }

    async fn stash_pop(&self) -> Result<()> {
        Err(anyhow!(
            "Not implemented for GitHub provider (use LocalGitProvider)"
        ))
    }

    async fn push(&self, _force: bool) -> Result<()> {
        Err(anyhow!(
            "Not implemented for GitHub provider (use LocalGitProvider)"
        ))
    }

    async fn pull(&self) -> Result<()> {
        Err(anyhow!(
            "Not implemented for GitHub provider (use LocalGitProvider)"
        ))
    }

    async fn fetch(&self) -> Result<()> {
        Err(anyhow!(
            "Not implemented for GitHub provider (use LocalGitProvider)"
        ))
    }

    async fn commit(&self, _message: &str, _all: bool, _amend: bool) -> Result<()> {
        Err(anyhow!(
            "Not implemented for GitHub provider (use LocalGitProvider)"
        ))
    }

    async fn discard_local_changes(&self) -> Result<()> {
        Err(anyhow!(
            "Not implemented for GitHub provider (use LocalGitProvider)"
        ))
    }

    async fn get_log(&self, _range: Option<&str>, _limit: Option<i32>) -> Result<String> {
        Err(anyhow!(
            "Not implemented for GitHub provider (use LocalGitProvider)"
        ))
    }

    async fn merge(&self, _source: &str) -> Result<()> {
        Err(anyhow!(
            "Not implemented for GitHub provider (use LocalGitProvider)"
        ))
    }

    async fn rebase(&self, _target: &str) -> Result<()> {
        Err(anyhow!(
            "Not implemented for GitHub provider (use LocalGitProvider)"
        ))
    }

    async fn check_submodule_status(&self, _path: &str) -> Result<bool> {
        Err(anyhow!(
            "Not implemented for GitHub provider (use LocalGitProvider)"
        ))
    }

    async fn update_submodule_pointer(&self, _path: &str) -> Result<()> {
        Err(anyhow!(
            "Not implemented for GitHub provider (use LocalGitProvider)"
        ))
    }

    async fn get_pull_request_threads(
        &self,
        _repository: &str,
        id: &str,
    ) -> Result<Vec<PullRequestThread>> {
        let line_comments_url = self.repo_url(&format!("/pulls/{}/comments", id));
        let line_resp = self.client.get(&line_comments_url).send().await?;
        line_resp.error_for_status_ref()?;
        let line_body: Value = line_resp.json().await?;

        let general_comments_url = self.repo_url(&format!("/issues/{}/comments", id));
        let general_resp = self.client.get(&general_comments_url).send().await?;
        general_resp.error_for_status_ref()?;
        let general_body: Value = general_resp.json().await?;

        let mut threads: Vec<PullRequestThread> = Vec::new();
        let mut thread_map: std::collections::HashMap<String, PullRequestThread> =
            std::collections::HashMap::new();
        let mut replies_map: std::collections::HashMap<String, Vec<PullRequestComment>> =
            std::collections::HashMap::new();

        if let Some(line_comments) = line_body.as_array() {
            for comment in line_comments {
                let comment_id = comment["id"].to_string();
                let in_reply_to = comment["in_reply_to_id"].as_i64().map(|id| id.to_string());

                let thread_id = in_reply_to.clone().unwrap_or_else(|| comment_id.clone());

                let pc = PullRequestComment {
                    id: comment_id.clone(),
                    author: comment["user"]["login"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    created_at: comment["created_at"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    content: comment["body"].as_str().unwrap_or_default().to_string(),
                    created_at_date: None,
                    created_at_time: None,
                    replies: vec![],
                };

                if in_reply_to.is_some() {
                    replies_map.entry(thread_id).or_default().push(pc);
                } else {
                    thread_map.insert(
                        comment_id,
                        PullRequestThread {
                            id: thread_id.clone(),
                            status: "active".to_string(),
                            file_path: comment["path"].as_str().map(|s| s.to_string()),
                            line: comment["line"].as_u64().map(|l| l as u32),
                            author: pc.author,
                            created_at: pc.created_at,
                            created_at_date: None,
                            created_at_time: None,
                            content: pc.content,
                            replies: vec![],
                        },
                    );
                }
            }
        }

        if let Some(general_comments) = general_body.as_array() {
            for comment in general_comments {
                let comment_id = comment["id"].to_string();

                let pc = PullRequestComment {
                    id: comment_id.clone(),
                    author: comment["user"]["login"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    created_at: comment["created_at"]
                        .as_str()
                        .unwrap_or_default()
                        .to_string(),
                    content: comment["body"].as_str().unwrap_or_default().to_string(),
                    created_at_date: None,
                    created_at_time: None,
                    replies: vec![],
                };

                thread_map.insert(
                    comment_id.clone(),
                    PullRequestThread {
                        id: comment_id,
                        status: "active".to_string(),
                        file_path: None,
                        line: None,
                        author: pc.author,
                        created_at: pc.created_at,
                        created_at_date: None,
                        created_at_time: None,
                        content: pc.content,
                        replies: vec![],
                    },
                );
            }
        }

        for (thread_id, mut thread) in thread_map {
            if let Some(replies) = replies_map.remove(&thread_id) {
                thread.replies = replies;
            }
            threads.push(thread);
        }

        Ok(threads)
    }

    async fn reply_to_pull_request_thread(
        &self,
        _repository: &str,
        pr_id: &str,
        thread_id: &str,
        message: &str,
    ) -> Result<()> {
        let in_reply_to = thread_id.parse::<u64>().unwrap_or(0);
        let payload = json!({
            "body": message,
            "in_reply_to": in_reply_to,
        });

        let url = self.repo_url(&format!("/pulls/{}/comments", pr_id));
        let resp = self.client.post(&url).json(&payload).send().await?;
        resp.error_for_status_ref()?;
        Ok(())
    }

    async fn update_pull_request_thread_status(
        &self,
        _repository: &str,
        _pr_id: &str,
        _thread_id: &str,
        _status: &str,
    ) -> Result<()> {
        eprintln!("update_pull_request_thread_status: GraphQL not implemented for GitHub provider");
        Ok(())
    }

    async fn get_pull_request_changed_files(
        &self,
        _repository: &str,
        pr_id: &str,
    ) -> Result<Vec<ChangedFile>> {
        let url = self.repo_url(&format!("/pulls/{}/files", pr_id));
        let resp = self.client.get(&url).send().await?;
        resp.error_for_status_ref()?;
        let body: Value = resp.json().await?;

        let files = body.as_array().ok_or_else(|| anyhow!("Not an array"))?;
        let mut results = Vec::new();

        for f in files {
            results.push(ChangedFile {
                path: f["filename"].as_str().unwrap_or_default().to_string(),
                change_type: f["status"].as_str().unwrap_or("modified").to_string(),
            });
        }

        Ok(results)
    }

    async fn add_pull_request_thread(
        &self,
        _repository: &str,
        pr_id: &str,
        content: &str,
        file_path: Option<&str>,
        line: Option<u32>,
    ) -> Result<PullRequestThread> {
        if let (Some(file), Some(line_num)) = (file_path, line) {
            let payload = json!({
                "event": "COMMENT",
                "comments": [
                    {
                        "path": file,
                        "line": line_num,
                        "body": content,
                    }
                ],
            });

            let url = self.repo_url(&format!("/pulls/{}/reviews", pr_id));
            let resp = self.client.post(&url).json(&payload).send().await?;
            resp.error_for_status_ref()?;
            let body: Value = resp.json().await?;

            Ok(PullRequestThread {
                id: body["id"].to_string(),
                status: "active".to_string(),
                file_path: Some(file.to_string()),
                line: Some(line_num),
                author: body["user"]["login"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                created_at: body["submitted_at"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                created_at_date: None,
                created_at_time: None,
                content: content.to_string(),
                replies: vec![],
            })
        } else {
            let payload = json!({
                "body": content,
            });

            let url = self.repo_url(&format!("/issues/{}/comments", pr_id));
            let resp = self.client.post(&url).json(&payload).send().await?;
            resp.error_for_status_ref()?;
            let body: Value = resp.json().await?;

            Ok(PullRequestThread {
                id: body["id"].to_string(),
                status: "active".to_string(),
                file_path: None,
                line: None,
                author: body["user"]["login"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                created_at: body["created_at"].as_str().unwrap_or_default().to_string(),
                created_at_date: None,
                created_at_time: None,
                content: content.to_string(),
                replies: vec![],
            })
        }
    }

    fn pull_request_url(&self, _repository: &str, id: &str) -> Result<String> {
        let base = self.web_base_url();
        Ok(format!("{}/{}/{}/pull/{}", base, self.owner, self.repo, id))
    }
}

#[async_trait]
impl PipelineProvider for GitHubProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            draft_pull_requests: true,
            pipeline_support: true,
            work_item_hierarchy: false,
            formal_artifact_links: false,
            merge_strategies: vec![
                MergeStrategy::Squash,
                MergeStrategy::Rebase,
                MergeStrategy::RebaseMerge,
                MergeStrategy::NoFastForward,
            ],
            work_item_relations: vec!["relates_to".to_string(), "blocks".to_string()],
        }
    }

    async fn list_pipelines(&self) -> Result<Vec<Pipeline>> {
        let url = self.repo_url("/actions/workflows");
        let resp = self.client.get(&url).send().await?;
        resp.error_for_status_ref()?;
        let body: Value = resp.json().await?;

        let workflows = body["workflows"]
            .as_array()
            .ok_or_else(|| anyhow!("No workflows"))?;
        let mut results = Vec::new();

        for w in workflows {
            results.push(Pipeline {
                id: w["id"].to_string(),
                name: w["name"].as_str().unwrap_or_default().to_string(),
                folder: String::new(),
            });
        }

        Ok(results)
    }

    async fn run_pipeline(&self, pipeline_id: &str, branch: &str) -> Result<PipelineRun> {
        let payload = json!({
            "ref": branch,
        });

        let url = self.repo_url(&format!("/actions/workflows/{}/dispatches", pipeline_id));
        let resp = self.client.post(&url).json(&payload).send().await?;
        resp.error_for_status_ref()?;

        // Fetch the latest run for this branch
        if let Ok(Some(run)) = self.get_latest_run(branch).await {
            return Ok(run);
        }

        Ok(PipelineRun {
            id: "unknown".to_string(),
            status: "queued".to_string(),
            result: None,
            url: String::new(),
        })
    }

    async fn get_latest_run(&self, branch: &str) -> Result<Option<PipelineRun>> {
        let url = self.repo_url("/actions/runs");
        let resp = self
            .client
            .get(&url)
            .query(&[("branch", branch), ("per_page", "1")])
            .send()
            .await?;
        resp.error_for_status_ref()?;
        let body: Value = resp.json().await?;

        let runs = body["workflow_runs"]
            .as_array()
            .ok_or_else(|| anyhow!("No runs"))?;

        if let Some(run) = runs.first() {
            return Ok(Some(self.parse_pipeline_run(run)?));
        }

        Ok(None)
    }

    async fn get_run_status(&self, run_id: &str) -> Result<PipelineRun> {
        let url = self.repo_url(&format!("/actions/runs/{}", run_id));
        let resp = self.client.get(&url).send().await?;
        resp.error_for_status_ref()?;
        let body: Value = resp.json().await?;
        self.parse_pipeline_run(&body)
    }
}

impl GitHubProvider {
    fn parse_pipeline_run(&self, body: &Value) -> Result<PipelineRun> {
        let status = match body["status"].as_str() {
            Some("queued") | Some("in_progress") => "inProgress",
            Some("completed") => "completed",
            _ => "unknown",
        };

        let result = match body["conclusion"].as_str() {
            Some("success") => Some("succeeded".to_string()),
            Some("failure") => Some("failed".to_string()),
            Some(other) => Some(other.to_string()),
            None => None,
        };

        Ok(PipelineRun {
            id: body["id"].to_string(),
            status: status.to_string(),
            result,
            url: body["html_url"].as_str().unwrap_or_default().to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Server, ServerGuard};

    async fn setup_mock_server() -> (ServerGuard, GitHubProvider) {
        let server = Server::new_async().await;
        let config = crate::core::config::GitHubConfig {
            token: Some("test-token".to_string()),
            owner: "test-owner".to_string(),
            repo: "test-repo".to_string(),
            base_url: Some(server.url()),
            client_id: None,
            app_id: None,
            account: "default".to_string(),
        };
        let provider = GitHubProvider::new(&config).unwrap();
        (server, provider)
    }

    // IssueTracker Tests

    #[tokio::test]
    async fn test_get_work_item() {
        let (mut server, provider) = setup_mock_server().await;
        let mock = server
            .mock("GET", "/repos/test-owner/test-repo/issues/42")
            .with_status(200)
            .with_body(
                json!({
                    "number": 42,
                    "title": "Fix bug",
                    "state": "open",
                    "labels": [{"name": "type:bug"}],
                    "assignee": {"login": "alice"}
                })
                .to_string(),
            )
            .create_async()
            .await;

        let result = provider
            .get_work_item(&WorkItemId::from_int(42))
            .await
            .unwrap();
        assert_eq!(result.id.as_str(), "42");
        assert_eq!(result.title, "Fix bug");
        assert_eq!(result.work_item_type, "Bug");
        assert_eq!(result.state, "open");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_create_work_item() {
        let (mut server, provider) = setup_mock_server().await;
        let me = server
            .mock("GET", "/user")
            .with_status(200)
            .with_body(json!({ "login": "alice" }).to_string())
            .create_async()
            .await;
        let mock = server
            .mock("POST", "/repos/test-owner/test-repo/issues")
            .with_status(201)
            .with_body(
                json!({
                    "number": 99,
                    "title": "New task",
                    "state": "open",
                    "labels": [{"name": "type:task"}],
                    "assignee": {"login": "alice"}
                })
                .to_string(),
            )
            .create_async()
            .await;

        let result = provider
            .create_work_item("New task", "Task", None, None, None)
            .await
            .unwrap();
        assert_eq!(result.title, "New task");
        assert_eq!(result.work_item_type, "Task");
        assert_eq!(result.assigned_to.as_deref(), Some("alice"));
        me.assert_async().await;
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_update_work_item() {
        let (mut server, provider) = setup_mock_server().await;
        let mock = server
            .mock("PATCH", "/repos/test-owner/test-repo/issues/10")
            .with_status(200)
            .with_body(
                json!({
                    "number": 10,
                    "title": "Updated",
                    "state": "open",
                    "labels": [{"name": "type:bug"}],
                    "assignee": null
                })
                .to_string(),
            )
            .create_async()
            .await;

        let result = provider
            .update_work_item(&WorkItemId::from_int(10), Some("Updated"), None, None, None)
            .await
            .unwrap();
        assert_eq!(result.title, "Updated");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_update_work_item_state() {
        let (mut server, provider) = setup_mock_server().await;
        let mock = server
            .mock("PATCH", "/repos/test-owner/test-repo/issues/5")
            .with_status(200)
            .with_body(
                json!({
                    "number": 5,
                    "title": "Test issue",
                    "state": "closed",
                    "labels": [],
                    "assignee": null
                })
                .to_string(),
            )
            .create_async()
            .await;

        let result = provider
            .update_work_item_state(&WorkItemId::from_int(5), "closed")
            .await
            .unwrap();
        assert_eq!(result.state, "closed");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_query_work_items() {
        let (mut server, provider) = setup_mock_server().await;
        let mock = server
            .mock(
                "GET",
                mockito::Matcher::Regex("/search/issues.*".to_string()),
            )
            .with_status(200)
            .with_body(
                json!({
                    "items": [{
                        "number": 1,
                        "title": "Test issue",
                        "state": "open",
                        "labels": [{"name": "type:bug"}],
                        "assignee": null
                    }],
                    "total_count": 1
                })
                .to_string(),
            )
            .create_async()
            .await;

        let filter = WorkItemFilter::default();
        let results = provider.query_work_items(&filter).await.unwrap();
        assert_eq!(results.len(), 1);
        mock.assert_async().await;
    }

    // VCSProvider Tests

    #[tokio::test]
    async fn test_get_pull_request_by_branch() {
        let (mut server, provider) = setup_mock_server().await;
        let mock = server
            .mock(
                "GET",
                mockito::Matcher::Regex("/repos/test-owner/test-repo/pulls.*".to_string()),
            )
            .with_status(200)
            .with_body(
                json!([{
                    "number": 7,
                    "title": "My PR",
                    "state": "open",
                    "head": {"ref": "feature/x"},
                    "base": {"ref": "main"},
                    "draft": false,
                    "user": {"login": "bob"},
                    "created_at": "2025-01-01T00:00:00Z"
                }])
                .to_string(),
            )
            .create_async()
            .await;

        let result = provider
            .get_pull_request_by_branch("", "feature/x")
            .await
            .unwrap();
        assert!(result.is_some());
        let pr = result.unwrap();
        assert_eq!(pr.id, "7");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_pull_request_details() {
        let (mut server, provider) = setup_mock_server().await;
        let mock = server
            .mock("GET", "/repos/test-owner/test-repo/pulls/7")
            .with_status(200)
            .with_body(
                json!({
                    "number": 7,
                    "title": "My PR",
                    "state": "open",
                    "head": {"ref": "feature/x"},
                    "base": {"ref": "main"},
                    "draft": false,
                    "user": {"login": "bob"},
                    "created_at": "2025-01-01T00:00:00Z"
                })
                .to_string(),
            )
            .create_async()
            .await;

        let result = provider.get_pull_request_details("", "7").await.unwrap();
        assert_eq!(result.title, "My PR");
        assert_eq!(result.status, "open");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_create_pull_request() {
        let (mut server, provider) = setup_mock_server().await;
        let mock = server
            .mock("POST", "/repos/test-owner/test-repo/pulls")
            .with_status(201)
            .with_body(
                json!({
                    "number": 1,
                    "title": "PR title",
                    "state": "open",
                    "head": {"ref": "feature/abc"},
                    "base": {"ref": "main"},
                    "draft": true,
                    "user": {"login": "alice"},
                    "created_at": "2025-01-01T00:00:00Z"
                })
                .to_string(),
            )
            .create_async()
            .await;

        let result = provider
            .create_pull_request("", "feature/abc", "main", "PR title", "desc", true, &[])
            .await
            .unwrap();
        assert_eq!(result.id, "1");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_complete_pull_request() {
        let (mut server, provider) = setup_mock_server().await;
        let mock = server
            .mock("PUT", "/repos/test-owner/test-repo/pulls/3/merge")
            .with_status(200)
            .with_body(json!({"message": "Pull Request successfully merged"}).to_string())
            .create_async()
            .await;

        let result = provider
            .complete_pull_request("", "3", MergeStrategy::Squash, false)
            .await;
        assert!(result.is_ok());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_add_reviewer() {
        let (mut server, provider) = setup_mock_server().await;
        let mock = server
            .mock(
                "POST",
                "/repos/test-owner/test-repo/pulls/5/requested_reviewers",
            )
            .with_status(200)
            .with_body(
                json!({
                    "number": 5,
                    "requested_reviewers": [{"login": "carol"}]
                })
                .to_string(),
            )
            .create_async()
            .await;

        let result = provider.add_reviewer("", "5", "carol").await;
        assert!(result.is_ok());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_create_branch() {
        let (mut server, provider) = setup_mock_server().await;
        let mock_get = server
            .mock("GET", "/repos/test-owner/test-repo/git/ref/heads/main")
            .with_status(200)
            .with_body(json!({"object": {"sha": "abc123"}}).to_string())
            .create_async()
            .await;
        let mock_post = server
            .mock("POST", "/repos/test-owner/test-repo/git/refs")
            .with_status(201)
            .with_body(
                json!({"ref": "refs/heads/feature/new", "object": {"sha": "abc123"}}).to_string(),
            )
            .create_async()
            .await;

        let result = provider.create_branch("", "feature/new", "main").await;
        assert!(result.is_ok());
        mock_get.assert_async().await;
        mock_post.assert_async().await;
    }

    #[tokio::test]
    async fn test_delete_branch() {
        let (mut server, provider) = setup_mock_server().await;
        let mock = server
            .mock(
                "DELETE",
                "/repos/test-owner/test-repo/git/refs/heads/feature/old",
            )
            .with_status(204)
            .create_async()
            .await;

        let result = provider.delete_branch("", "feature/old").await;
        assert!(result.is_ok());
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_pull_request_changed_files() {
        let (mut server, provider) = setup_mock_server().await;
        let mock = server
            .mock("GET", "/repos/test-owner/test-repo/pulls/8/files")
            .with_status(200)
            .with_body(
                json!([
                    {"filename": "src/main.rs", "status": "modified"},
                    {"filename": "README.md", "status": "added"}
                ])
                .to_string(),
            )
            .create_async()
            .await;

        let result = provider
            .get_pull_request_changed_files("", "8")
            .await
            .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].path, "src/main.rs");
        mock.assert_async().await;
    }

    // PipelineProvider Tests

    #[tokio::test]
    async fn test_list_pipelines() {
        let (mut server, provider) = setup_mock_server().await;
        let mock = server
            .mock("GET", "/repos/test-owner/test-repo/actions/workflows")
            .with_status(200)
            .with_body(
                json!({
                    "workflows": [{
                        "id": 100,
                        "name": "CI",
                        "path": ".github/workflows/ci.yml"
                    }]
                })
                .to_string(),
            )
            .create_async()
            .await;

        let result = provider.list_pipelines().await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "CI");
        assert_eq!(result[0].id, "100");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn test_get_run_status() {
        let (mut server, provider) = setup_mock_server().await;
        let mock = server
            .mock("GET", "/repos/test-owner/test-repo/actions/runs/999")
            .with_status(200)
            .with_body(
                json!({
                    "id": 999,
                    "status": "completed",
                    "conclusion": "success",
                    "html_url": "https://github.com/runs/999"
                })
                .to_string(),
            )
            .create_async()
            .await;

        let result = provider.get_run_status("999").await.unwrap();
        assert_eq!(result.status, "completed");
        assert_eq!(result.result, Some("succeeded".to_string()));
        mock.assert_async().await;
    }
}
