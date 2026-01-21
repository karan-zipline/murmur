use fugue_protocol::{
    CommitListRequest, CommitListResponse, CommitRecord as ProtoCommitRecord, Request, Response,
    MSG_COMMIT_LIST,
};

use super::super::SharedState;
use super::error_response;

pub(in crate::daemon) async fn handle_commit_list(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<CommitListRequest, _> = serde_json::from_value(payload);
    let list = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let limit = list.limit.unwrap_or(50) as usize;

    let commits = {
        let commits = shared.commits.lock().await;
        commits
            .iter()
            .filter(|(project, _)| match list.project.as_deref() {
                None => true,
                Some(filter) => filter == project.as_str(),
            })
            .flat_map(|(project, log)| {
                let project = project.clone();
                log.list_recent(limit)
                    .into_iter()
                    .map(move |c| ProtoCommitRecord {
                        project: project.clone(),
                        sha: c.sha,
                        branch: c.branch,
                        agent_id: c.agent_id,
                        issue_id: c.issue_id,
                        merged_at_ms: c.merged_at_ms,
                    })
            })
            .collect::<Vec<_>>()
    };

    let payload = CommitListResponse { commits };

    Response {
        r#type: MSG_COMMIT_LIST.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    }
}
