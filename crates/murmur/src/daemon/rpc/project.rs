use std::sync::Arc;

use murmur_core::config::{
    AgentBackend, IssueBackend, MergeStrategy, PermissionsChecker, ProjectConfig,
};
use murmur_protocol::{
    ProjectAddRequest, ProjectAddResponse, ProjectConfigGetRequest, ProjectConfigGetResponse,
    ProjectConfigSetRequest, ProjectConfigShowRequest, ProjectConfigShowResponse, ProjectInfo,
    ProjectListResponse, ProjectRemoveRequest, ProjectStatusRequest, ProjectStatusResponse,
    Request, Response, MSG_PROJECT_ADD, MSG_PROJECT_CONFIG_GET, MSG_PROJECT_CONFIG_SET,
    MSG_PROJECT_CONFIG_SHOW, MSG_PROJECT_LIST, MSG_PROJECT_REMOVE, MSG_PROJECT_STATUS,
};

use super::super::orchestration::orchestrator_is_running;
use super::super::{persist_agents_runtime, project_dir, project_repo_dir, SharedState};
use super::error_response;

use crate::config_store;

fn normalize_remote_url(url: &str) -> String {
    let mut s = url.trim().to_owned();
    while s.ends_with('/') {
        s.pop();
    }
    if let Some(stripped) = s.strip_suffix(".git") {
        s = stripped.to_owned();
    }
    s
}

fn remote_urls_match(a: &str, b: &str) -> bool {
    normalize_remote_url(a) == normalize_remote_url(b)
}

pub(in crate::daemon) async fn handle_project_list(shared: &SharedState, req: Request) -> Response {
    let cfg = shared.config.lock().await;
    let project_cfgs = cfg.projects.clone();
    drop(cfg);

    let mut projects = Vec::with_capacity(project_cfgs.len());
    for p in project_cfgs {
        let running = orchestrator_is_running(shared, &p.name).await;
        projects.push(ProjectInfo {
            name: p.name.clone(),
            remote_url: p.remote_url.clone(),
            repo_dir: project_repo_dir(&shared.paths, &p.name)
                .to_string_lossy()
                .to_string(),
            max_agents: p.max_agents,
            running,
            backend: format!("{:?}", p.effective_coding_backend()).to_ascii_lowercase(),
        });
    }

    let payload = ProjectListResponse { projects };

    Response {
        r#type: MSG_PROJECT_LIST.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    }
}

pub(in crate::daemon) async fn handle_project_add(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<ProjectAddRequest, _> = serde_json::from_value(payload);
    let add = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let mut cfg = shared.config.lock().await;
    if cfg.project(&add.name).is_some() {
        return error_response(req, "project already exists");
    }

    let max_agents = add.max_agents.unwrap_or(3);
    let autostart = add.autostart.unwrap_or(false);
    let agent_backend = match add
        .backend
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some("claude") => AgentBackend::Claude,
        Some("codex") => AgentBackend::Codex,
        Some(other) => return error_response(req, &format!("unknown backend: {other}")),
        None => AgentBackend::Codex,
    };

    let project = ProjectConfig {
        name: add.name.clone(),
        remote_url: add.remote_url.clone(),
        max_agents,
        issue_backend: IssueBackend::Tk,
        permissions_checker: PermissionsChecker::Manual,
        agent_backend,
        planner_backend: None,
        coding_backend: None,
        merge_strategy: MergeStrategy::Direct,
        allowed_authors: vec![],
        autostart,
        linear_team: None,
        linear_project: None,
        extra: Default::default(),
    };

    let next_cfg = match cfg.add_project(project) {
        Ok(next) => next,
        Err(err) => return error_response(req, &err.to_string()),
    };

    let repo_dir = project_repo_dir(&shared.paths, &add.name);
    if repo_dir.exists() {
        let meta = match tokio::fs::metadata(&repo_dir).await {
            Ok(v) => v,
            Err(err) => {
                return error_response(
                    req,
                    &format!("stat repo directory {}: {err}", repo_dir.display()),
                );
            }
        };
        if !meta.is_dir() {
            return error_response(
                req,
                &format!(
                    "repo directory already exists but is not a directory: {}",
                    repo_dir.display()
                ),
            );
        }

        let existing = match shared.git.remote_origin_url(&repo_dir).await {
            Ok(v) => v,
            Err(err) => {
                return error_response(
                    req,
                    &format!(
                        "repo directory already exists but is not a usable git repo: {} ({err:#})",
                        repo_dir.display()
                    ),
                );
            }
        };

        if !remote_urls_match(&existing, &add.remote_url) {
            return error_response(
                req,
                &format!(
                    "repo directory already exists with different origin remote: existing={existing} requested={}",
                    add.remote_url
                ),
            );
        }
    } else {
        if let Some(parent) = repo_dir.parent() {
            if let Err(err) = tokio::fs::create_dir_all(parent).await {
                return error_response(req, &format!("create project dir: {err}"));
            }
        }

        if let Err(err) = shared.git.clone_repo(&add.remote_url, &repo_dir).await {
            let _ = tokio::fs::remove_dir_all(project_dir(&shared.paths, &add.name)).await;
            return error_response(req, &format!("git clone failed: {err:#}"));
        }
    }

    if let Err(err) = config_store::save(&shared.paths, &next_cfg).await {
        return error_response(req, &format!("save config failed: {err:#}"));
    }

    *cfg = next_cfg;

    let payload = ProjectAddResponse {
        name: add.name,
        remote_url: add.remote_url,
        repo_dir: repo_dir.to_string_lossy().to_string(),
        max_agents,
    };

    Response {
        r#type: MSG_PROJECT_ADD.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    }
}

pub(in crate::daemon) async fn handle_project_remove(
    shared: Arc<SharedState>,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<ProjectRemoveRequest, _> = serde_json::from_value(payload);
    let remove = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    // Stop orchestration for this project (best-effort).
    let runtime = {
        let mut orchestrators = shared.orchestrators.lock().await;
        orchestrators.remove(&remove.name)
    };
    if let Some(rt) = runtime {
        let _ = rt.shutdown_tx.send(true);
        tokio::spawn(async move {
            let mut task = rt.task;
            if tokio::time::timeout(std::time::Duration::from_secs(3), &mut task)
                .await
                .is_err()
            {
                task.abort();
                let _ = tokio::time::timeout(std::time::Duration::from_secs(3), task).await;
            }
        });
    }

    // Abort all agents for this project.
    let agent_runtimes = {
        let mut agents = shared.agents.lock().await;
        let ids = agents
            .agents
            .iter()
            .filter(|(_, rt)| rt.record.project == remove.name)
            .map(|(id, _)| id.to_owned())
            .collect::<Vec<_>>();

        let mut removed = Vec::new();
        for id in ids {
            if let Some(rt) = agents.agents.remove(&id) {
                removed.push((id, rt));
            }
        }
        removed
    };

    for (agent_id, mut rt) in agent_runtimes {
        let _ = rt.abort_tx.send(true);
        for task in rt.tasks.drain(..) {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(3), task).await;
        }

        // Release any claims tied to this agent ID.
        {
            let mut claims = shared.claims.lock().await;
            *claims = claims.release_by_agent(&agent_id);
        }
    }

    {
        let mut claims = shared.claims.lock().await;
        let entries = claims.list();
        let mut next = claims.clone();
        for entry in entries {
            if entry.project == remove.name {
                next = next.release(&entry.project, &entry.issue_id);
            }
        }
        *claims = next;
    }

    {
        let mut pending = shared.pending_permissions.lock().await;
        let _ = pending.cancel_for_project(&remove.name);
    }
    {
        let mut pending = shared.pending_questions.lock().await;
        let _ = pending.cancel_for_project(&remove.name);
    }
    {
        let mut completed = shared.completed_issues.lock().await;
        completed.remove(&remove.name);
    }
    {
        let mut commits = shared.commits.lock().await;
        commits.remove(&remove.name);
    }

    // Optionally delete all worktrees for this project, leaving the repo intact.
    if remove.delete_worktrees {
        let worktrees_dir = project_dir(&shared.paths, &remove.name).join("worktrees");

        if worktrees_dir.exists() {
            let wtm = crate::worktrees::WorktreeManager::new(&shared.git, &shared.paths);
            match tokio::fs::read_dir(&worktrees_dir).await {
                Ok(mut dir) => loop {
                    let entry = match dir.next_entry().await {
                        Ok(Some(v)) => v,
                        Ok(None) => break,
                        Err(err) => {
                            tracing::warn!(
                                project = %remove.name,
                                error = %err,
                                "failed to read worktrees dir entry"
                            );
                            break;
                        }
                    };

                    let path = entry.path();
                    let ty = entry.file_type().await;
                    if !ty.is_ok_and(|t| t.is_dir()) {
                        continue;
                    }

                    if let Err(err) = wtm.remove_worktree(&remove.name, &path).await {
                        tracing::warn!(
                            project = %remove.name,
                            worktree_dir = %path.display(),
                            error = %err,
                            "failed to remove worktree"
                        );
                        let _ = tokio::fs::remove_dir_all(&path).await;
                    }
                },
                Err(err) => tracing::warn!(
                    project = %remove.name,
                    error = %err,
                    "failed to read worktrees dir"
                ),
            }
        }
    }

    persist_agents_runtime(shared.clone()).await;

    let mut cfg = shared.config.lock().await;
    let next_cfg = match cfg.remove_project(&remove.name) {
        Ok(next) => next,
        Err(err) => return error_response(req, &err.to_string()),
    };

    if let Err(err) = config_store::save(&shared.paths, &next_cfg).await {
        return error_response(req, &format!("save config failed: {err:#}"));
    }

    *cfg = next_cfg;

    Response {
        r#type: MSG_PROJECT_REMOVE.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::Value::Null,
    }
}

pub(in crate::daemon) async fn handle_project_config_show(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<ProjectConfigShowRequest, _> = serde_json::from_value(payload);
    let show = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let cfg = shared.config.lock().await;
    let map = match cfg.project_config_map(&show.name) {
        Ok(v) => v,
        Err(err) => return error_response(req, &err.to_string()),
    };

    let mut json_map = serde_json::Map::new();
    for (k, v) in map {
        let json_value = serde_json::to_value(v).unwrap_or(serde_json::Value::Null);
        json_map.insert(k, json_value);
    }

    let payload = ProjectConfigShowResponse {
        name: show.name,
        config: json_map,
    };

    Response {
        r#type: MSG_PROJECT_CONFIG_SHOW.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    }
}

pub(in crate::daemon) async fn handle_project_config_get(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<ProjectConfigGetRequest, _> = serde_json::from_value(payload);
    let get = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let cfg = shared.config.lock().await;
    let v = match cfg.get_project_key_value(&get.name, &get.key) {
        Ok(v) => v,
        Err(err) => return error_response(req, &err.to_string()),
    };

    let payload = ProjectConfigGetResponse {
        name: get.name,
        key: get.key,
        value: serde_json::to_value(v).unwrap_or(serde_json::Value::Null),
    };

    Response {
        r#type: MSG_PROJECT_CONFIG_GET.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    }
}

pub(in crate::daemon) async fn handle_project_config_set(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<ProjectConfigSetRequest, _> = serde_json::from_value(payload);
    let set = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let mut cfg = shared.config.lock().await;
    let next_cfg = match cfg.set_project_key(&set.name, &set.key, &set.value) {
        Ok(next) => next,
        Err(err) => return error_response(req, &err.to_string()),
    };

    if let Err(err) = config_store::save(&shared.paths, &next_cfg).await {
        return error_response(req, &format!("save config failed: {err:#}"));
    }

    *cfg = next_cfg;

    Response {
        r#type: MSG_PROJECT_CONFIG_SET.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::Value::Null,
    }
}

pub(in crate::daemon) async fn handle_project_status(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<ProjectStatusRequest, _> = serde_json::from_value(payload);
    let status = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let cfg = shared.config.lock().await;
    let Some(project) = cfg.project(&status.name) else {
        return error_response(req, "project not found");
    };
    let configured = project.remote_url.clone();
    drop(cfg);

    let repo_dir = project_repo_dir(&shared.paths, &status.name);
    let socket_path = shared.paths.socket_path.clone();

    let repo_exists = repo_dir.join(".git").exists();
    let socket_reachable = true;

    let remote_url_actual = if repo_exists {
        shared.git.remote_origin_url(&repo_dir).await.ok()
    } else {
        None
    };

    let remote_matches = remote_url_actual
        .as_deref()
        .is_some_and(|actual| remote_urls_match(actual, &configured));

    let orchestrator_running = orchestrator_is_running(shared, &status.name).await;

    let payload = ProjectStatusResponse {
        name: status.name,
        repo_dir: repo_dir.to_string_lossy().to_string(),
        socket_path: socket_path.to_string_lossy().to_string(),
        repo_exists,
        socket_reachable,
        remote_url_configured: configured,
        remote_url_actual,
        remote_matches,
        orchestrator_running,
    };

    Response {
        r#type: MSG_PROJECT_STATUS.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    }
}
