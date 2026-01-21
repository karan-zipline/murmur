use std::collections::BTreeSet;

use fugue_protocol::{
    ClaimInfo, ClaimListRequest, ClaimListResponse, Request, Response, MSG_CLAIM_LIST,
};

use super::super::SharedState;
use super::error_response;

pub(in crate::daemon) async fn handle_claim_list(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<ClaimListRequest, _> = serde_json::from_value(payload);
    let list = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let claim_entries = {
        let claims = shared.claims.lock().await;
        claims.list()
    };

    let agent_ids = {
        let agents = shared.agents.lock().await;
        agents.agents.keys().cloned().collect::<BTreeSet<_>>()
    };

    let mut infos = claim_entries
        .into_iter()
        .filter(|c| agent_ids.contains(&c.agent_id))
        .filter(|c| match list.project.as_deref() {
            None => true,
            Some(p) => p == c.project.as_str(),
        })
        .map(|c| ClaimInfo {
            project: c.project,
            issue_id: c.issue_id,
            agent_id: c.agent_id,
        })
        .collect::<Vec<_>>();
    infos.sort_by(|a, b| a.project.cmp(&b.project).then(a.issue_id.cmp(&b.issue_id)));

    let payload = ClaimListResponse { claims: infos };

    Response {
        r#type: MSG_CLAIM_LIST.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    }
}
