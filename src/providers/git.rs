use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::process::Command;

use crate::core::models::{MergeStrategy, PullRequest, Repository};
use crate::providers::VCSProvider;

pub struct LocalGitProvider;

impl LocalGitProvider {
    pub fn run_git(&self, args: &[&str]) -> Result<String> {
        let output = Command::new("git").args(args).output()?;

        if !output.status.success() {
            return Err(anyhow!(
                "Git command failed: git {} - {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr)
            ));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

#[async_trait]
impl VCSProvider for LocalGitProvider {
    async fn get_pull_request_by_branch(
        &self,
        _repository: &str,
        _branch: &str,
    ) -> Result<Option<PullRequest>> {
        Err(anyhow!("Not implemented for local git"))
    }
    async fn get_pull_request_details(&self, _repository: &str, _id: i32) -> Result<PullRequest> {
        Err(anyhow!("Not implemented for local git"))
    }
    async fn create_pull_request(
        &self,
        _repository: &str,
        _source: &str,
        _target: &str,
        _title: &str,
        _description: &str,
        _is_draft: bool,
    ) -> Result<PullRequest> {
        Err(anyhow!("Not implemented for local git"))
    }
    async fn update_pull_request(
        &self,
        _repository: &str,
        _id: i32,
        _title: Option<&str>,
        _description: Option<&str>,
        _is_draft: Option<bool>,
        _status: Option<&str>,
    ) -> Result<PullRequest> {
        Err(anyhow!("Not implemented for local git"))
    }
    async fn complete_pull_request(
        &self,
        _repository: &str,
        _id: i32,
        _strategy: MergeStrategy,
        _delete_source_branch: bool,
    ) -> Result<()> {
        Err(anyhow!("Not implemented for local git"))
    }
    async fn add_reviewer(&self, _repository: &str, _id: i32, _reviewer_id: &str) -> Result<()> {
        Err(anyhow!("Not implemented for local git"))
    }
    async fn create_branch(&self, _repository: &str, _name: &str, _source: &str) -> Result<()> {
        Err(anyhow!("Not implemented for local git"))
    }
    async fn delete_branch(&self, _repository: &str, _name: &str) -> Result<()> {
        Err(anyhow!("Not implemented for local git"))
    }
    async fn get_repository(&self, _name: &str) -> Result<Repository> {
        Err(anyhow!("Not implemented for local git"))
    }

    async fn get_current_branch(&self) -> Result<String> {
        self.run_git(&["rev-parse", "--abbrev-ref", "HEAD"])
    }

    async fn checkout_branch(&self, name: &str) -> Result<()> {
        self.run_git(&["checkout", name])?;
        Ok(())
    }

    async fn get_status(&self) -> Result<String> {
        self.run_git(&["status", "--short"])
    }

    async fn stash_push(&self, message: &str) -> Result<()> {
        self.run_git(&["stash", "push", "-m", message])?;
        Ok(())
    }

    async fn stash_pop(&self) -> Result<()> {
        self.run_git(&["stash", "pop"])?;
        Ok(())
    }

    async fn push(&self, force: bool) -> Result<()> {
        let mut args = vec!["push"];
        if force {
            args.push("--force-with-lease");
        }
        self.run_git(&args)?;
        Ok(())
    }

    async fn pull(&self) -> Result<()> {
        self.run_git(&["pull"])?;
        Ok(())
    }

    async fn commit(&self, message: &str, all: bool) -> Result<()> {
        let mut args = vec!["commit", "-m", message];
        if all {
            args.push("-a");
        }
        self.run_git(&args)?;
        Ok(())
    }

    async fn check_submodule_status(&self, path: &str) -> Result<bool> {
        let output = self.run_git(&["submodule", "status", path])?;
        Ok(output.starts_with('+') || output.starts_with('U'))
    }

    async fn update_submodule_pointer(&self, path: &str) -> Result<()> {
        self.run_git(&["add", path])?;
        Ok(())
    }
}
