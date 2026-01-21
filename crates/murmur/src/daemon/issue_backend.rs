use murmur_core::config::IssueBackend;

use crate::github::GithubBackend;
use crate::issues::TkBackend;
use crate::linear::LinearBackend;
use crate::providers;

use super::{project_repo_dir, SharedState};

pub(in crate::daemon) enum IssueBackendImpl<'a> {
    Tk(TkBackend<'a>),
    Github(GithubBackend),
    Linear(LinearBackend),
}

impl<'a> IssueBackendImpl<'a> {
    pub(in crate::daemon) async fn get(
        &self,
        id: &str,
    ) -> anyhow::Result<murmur_core::issue::Issue> {
        match self {
            IssueBackendImpl::Tk(b) => b.get(id).await,
            IssueBackendImpl::Github(b) => b.get(id).await,
            IssueBackendImpl::Linear(b) => b.get(id).await,
        }
    }

    pub(in crate::daemon) async fn list(
        &self,
        filter: murmur_core::issue::ListFilter,
    ) -> anyhow::Result<Vec<murmur_core::issue::Issue>> {
        match self {
            IssueBackendImpl::Tk(b) => b.list(filter).await,
            IssueBackendImpl::Github(b) => b.list(filter).await,
            IssueBackendImpl::Linear(b) => b.list(filter).await,
        }
    }

    pub(in crate::daemon) async fn ready(&self) -> anyhow::Result<Vec<murmur_core::issue::Issue>> {
        match self {
            IssueBackendImpl::Tk(b) => b.ready().await,
            IssueBackendImpl::Github(b) => b.ready().await,
            IssueBackendImpl::Linear(b) => b.ready().await,
        }
    }

    pub(in crate::daemon) async fn create(
        &self,
        now_ms: u64,
        params: murmur_core::issue::CreateParams,
    ) -> anyhow::Result<murmur_core::issue::Issue> {
        match self {
            IssueBackendImpl::Tk(b) => b.create(now_ms, params).await,
            IssueBackendImpl::Github(b) => b.create(params).await,
            IssueBackendImpl::Linear(b) => b.create(params).await,
        }
    }

    pub(in crate::daemon) async fn update(
        &self,
        now_ms: u64,
        id: &str,
        params: murmur_core::issue::UpdateParams,
    ) -> anyhow::Result<murmur_core::issue::Issue> {
        match self {
            IssueBackendImpl::Tk(b) => b.update(now_ms, id, params).await,
            IssueBackendImpl::Github(b) => b.update(id, params).await,
            IssueBackendImpl::Linear(b) => b.update(id, params).await,
        }
    }

    pub(in crate::daemon) async fn close(&self, now_ms: u64, id: &str) -> anyhow::Result<()> {
        match self {
            IssueBackendImpl::Tk(b) => b.close(now_ms, id).await,
            IssueBackendImpl::Github(b) => b.close(id).await,
            IssueBackendImpl::Linear(b) => b.close(id).await,
        }
    }

    pub(in crate::daemon) async fn comment(
        &self,
        now_ms: u64,
        id: &str,
        body: &str,
    ) -> anyhow::Result<()> {
        match self {
            IssueBackendImpl::Tk(b) => b.comment(now_ms, id, body).await,
            IssueBackendImpl::Github(b) => b.comment(id, body).await,
            IssueBackendImpl::Linear(b) => b.comment(id, body).await,
        }
    }

    pub(in crate::daemon) async fn commit(&self, message: &str) -> anyhow::Result<()> {
        match self {
            IssueBackendImpl::Tk(b) => b.commit(message).await,
            IssueBackendImpl::Github(b) => b.commit().await,
            IssueBackendImpl::Linear(b) => b.commit().await,
        }
    }

    pub(in crate::daemon) async fn plan(
        &self,
        now_ms: u64,
        id: &str,
        plan_content: &str,
    ) -> anyhow::Result<()> {
        let issue = self.get(id).await?;
        let description = murmur_core::issue::upsert_plan_section(&issue.description, plan_content);
        let _ = self
            .update(
                now_ms,
                id,
                murmur_core::issue::UpdateParams {
                    description: Some(description),
                    ..Default::default()
                },
            )
            .await?;
        Ok(())
    }
}

pub(in crate::daemon) async fn issue_backend_for_project<'a>(
    shared: &'a SharedState,
    project: &str,
) -> Result<IssueBackendImpl<'a>, String> {
    let cfg = shared.config.lock().await;
    let Some(p) = cfg.project(project) else {
        return Err("project not found".to_owned());
    };
    let project_cfg = p.clone();

    let github_token = providers::github_token(&cfg);
    let github_url = providers::github_graphql_url(&cfg);
    let linear_api_key = providers::linear_api_key(&cfg);
    let linear_url = providers::linear_graphql_url(&cfg);
    drop(cfg);

    let repo_dir = project_repo_dir(&shared.paths, project);
    if !repo_dir.join(".git").exists() {
        return Err("project repo does not exist (run `project add`)".to_owned());
    }

    match project_cfg.issue_backend {
        IssueBackend::Tk => TkBackend::new(&shared.git, repo_dir)
            .await
            .map(IssueBackendImpl::Tk)
            .map_err(|e| e.to_string()),
        IssueBackend::Github | IssueBackend::Gh => {
            let Some(token) = github_token else {
                return Err(
                    "github token not set (set GITHUB_TOKEN/GH_TOKEN or [providers.github].token)"
                        .to_owned(),
                );
            };
            GithubBackend::from_repo(
                &shared.git,
                &repo_dir,
                token,
                project_cfg.allowed_authors.clone(),
                github_url,
            )
            .await
            .map(IssueBackendImpl::Github)
            .map_err(|e| format!("{e:#}"))
        }
        IssueBackend::Linear => {
            let Some(api_key) = linear_api_key else {
                return Err(
                    "linear api key not set (set LINEAR_API_KEY or [providers.linear].api-key)"
                        .to_owned(),
                );
            };
            let Some(team_id) = project_cfg
                .linear_team
                .as_ref()
                .and_then(|s| (!s.trim().is_empty()).then(|| s.to_owned()))
            else {
                return Err("linear-team is required for linear backend".to_owned());
            };
            let backend = LinearBackend::new(
                team_id,
                project_cfg.linear_project.clone(),
                api_key,
                project_cfg.allowed_authors.clone(),
                linear_url,
            )
            .map_err(|e| e.to_string())?;
            Ok(IssueBackendImpl::Linear(backend))
        }
    }
}
