use fugue_protocol::{
    IssueCommentRequest, IssueCommitRequest, IssueCreateRequest, IssueCreateResponse,
    IssueGetRequest, IssueGetResponse, IssueListRequest, IssueListResponse, IssuePlanRequest,
    IssueReadyRequest, IssueReadyResponse, IssueUpdateRequest, IssueUpdateResponse, Request,
    Response, MSG_ISSUE_CLOSE, MSG_ISSUE_COMMENT, MSG_ISSUE_COMMIT, MSG_ISSUE_CREATE,
    MSG_ISSUE_GET, MSG_ISSUE_LIST, MSG_ISSUE_PLAN, MSG_ISSUE_READY, MSG_ISSUE_UPDATE,
};

use super::super::{
    from_proto_issue_status, issue_backend_for_project, now_ms, to_proto_issue,
    to_proto_issue_summary, SharedState,
};
use super::error_response;

pub(in crate::daemon) async fn handle_issue_list(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<IssueListRequest, _> = serde_json::from_value(payload);
    let list = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let backend = match issue_backend_for_project(shared, &list.project).await {
        Ok(v) => v,
        Err(msg) => return error_response(req, &msg),
    };

    let mut issues = match backend.list(fugue_core::issue::ListFilter::default()).await {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("{err:#}")),
    };
    issues.sort_by(|a, b| a.id.cmp(&b.id));

    let payload = IssueListResponse {
        issues: issues.iter().map(to_proto_issue_summary).collect(),
    };

    Response {
        r#type: MSG_ISSUE_LIST.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    }
}

pub(in crate::daemon) async fn handle_issue_get(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<IssueGetRequest, _> = serde_json::from_value(payload);
    let get = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let backend = match issue_backend_for_project(shared, &get.project).await {
        Ok(v) => v,
        Err(msg) => return error_response(req, &msg),
    };

    let issue = match backend.get(&get.id).await {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("{err:#}")),
    };

    let payload = IssueGetResponse {
        issue: to_proto_issue(issue),
    };

    Response {
        r#type: MSG_ISSUE_GET.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    }
}

pub(in crate::daemon) async fn handle_issue_ready(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<IssueReadyRequest, _> = serde_json::from_value(payload);
    let ready = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let backend = match issue_backend_for_project(shared, &ready.project).await {
        Ok(v) => v,
        Err(msg) => return error_response(req, &msg),
    };

    let mut issues = match backend.ready().await {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("{err:#}")),
    };
    issues.sort_by(|a, b| a.id.cmp(&b.id));

    let payload = IssueReadyResponse {
        issues: issues.iter().map(to_proto_issue_summary).collect(),
    };

    Response {
        r#type: MSG_ISSUE_READY.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    }
}

pub(in crate::daemon) async fn handle_issue_create(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<IssueCreateRequest, _> = serde_json::from_value(payload);
    let create = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let backend = match issue_backend_for_project(shared, &create.project).await {
        Ok(v) => v,
        Err(msg) => return error_response(req, &msg),
    };

    let params = fugue_core::issue::CreateParams {
        title: create.title,
        description: create.description.unwrap_or_default(),
        issue_type: create.issue_type.unwrap_or_default(),
        priority: create.priority.unwrap_or(0),
        labels: create.labels,
        dependencies: create.dependencies,
        links: create.links,
    };

    let issue = match backend.create(now_ms(), params).await {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("{err:#}")),
    };

    let payload = IssueCreateResponse {
        issue: to_proto_issue(issue),
    };

    Response {
        r#type: MSG_ISSUE_CREATE.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    }
}

pub(in crate::daemon) async fn handle_issue_update(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<IssueUpdateRequest, _> = serde_json::from_value(payload);
    let update = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let backend = match issue_backend_for_project(shared, &update.project).await {
        Ok(v) => v,
        Err(msg) => return error_response(req, &msg),
    };

    let params = fugue_core::issue::UpdateParams {
        title: update.title,
        description: update.description,
        status: update.status.map(from_proto_issue_status),
        priority: update.priority,
        issue_type: update.issue_type,
        labels: update.labels,
        dependencies: update.dependencies,
        links: update.links,
    };

    let issue = match backend.update(now_ms(), &update.id, params).await {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("{err:#}")),
    };

    let payload = IssueUpdateResponse {
        issue: to_proto_issue(issue),
    };

    Response {
        r#type: MSG_ISSUE_UPDATE.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    }
}

pub(in crate::daemon) async fn handle_issue_close(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<fugue_protocol::IssueCloseRequest, _> = serde_json::from_value(payload);
    let close = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let backend = match issue_backend_for_project(shared, &close.project).await {
        Ok(v) => v,
        Err(msg) => return error_response(req, &msg),
    };

    if let Err(err) = backend.close(now_ms(), &close.id).await {
        return error_response(req, &format!("{err:#}"));
    }

    Response {
        r#type: MSG_ISSUE_CLOSE.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::Value::Null,
    }
}

pub(in crate::daemon) async fn handle_issue_comment(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<IssueCommentRequest, _> = serde_json::from_value(payload);
    let comment = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let backend = match issue_backend_for_project(shared, &comment.project).await {
        Ok(v) => v,
        Err(msg) => return error_response(req, &msg),
    };

    if let Err(err) = backend.comment(now_ms(), &comment.id, &comment.body).await {
        return error_response(req, &format!("{err:#}"));
    }

    Response {
        r#type: MSG_ISSUE_COMMENT.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::Value::Null,
    }
}

pub(in crate::daemon) async fn handle_issue_plan(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<IssuePlanRequest, _> = serde_json::from_value(payload);
    let plan = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    if plan.plan.trim().is_empty() {
        return error_response(req, "plan is required");
    }

    let backend = match issue_backend_for_project(shared, &plan.project).await {
        Ok(v) => v,
        Err(msg) => return error_response(req, &msg),
    };

    if let Err(err) = backend.plan(now_ms(), &plan.id, &plan.plan).await {
        return error_response(req, &format!("{err:#}"));
    }

    Response {
        r#type: MSG_ISSUE_PLAN.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::Value::Null,
    }
}

pub(in crate::daemon) async fn handle_issue_commit(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<IssueCommitRequest, _> = serde_json::from_value(payload);
    let commit = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let backend = match issue_backend_for_project(shared, &commit.project).await {
        Ok(v) => v,
        Err(msg) => return error_response(req, &msg),
    };

    if let Err(err) = backend.commit("issue: update tickets").await {
        return error_response(req, &format!("{err:#}"));
    }

    Response {
        r#type: MSG_ISSUE_COMMIT.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::Value::Null,
    }
}
