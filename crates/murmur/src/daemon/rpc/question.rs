use std::sync::atomic::Ordering;
use std::sync::Arc;

use murmur_protocol::{
    Request, Response, UserQuestion, UserQuestionListRequest, UserQuestionListResponse,
    UserQuestionRequestPayload, UserQuestionRespondPayload, UserQuestionResponse,
    EVT_USER_QUESTION, MSG_QUESTION_LIST, MSG_QUESTION_REQUEST, MSG_QUESTION_RESPOND,
};
use tokio::sync::oneshot;

use super::super::{now_ms, SharedState};
use super::error_response;

fn generate_request_id() -> String {
    let mut buf = [0u8; 4];
    if getrandom::getrandom(&mut buf).is_ok() {
        let mut out = String::with_capacity(8);
        for b in buf {
            use std::fmt::Write as _;
            let _ = write!(&mut out, "{b:02x}");
        }
        return out;
    }
    let nonce = now_ms();
    format!("q-{nonce}")
}

pub(in crate::daemon) async fn handle_question_request(
    shared: Arc<SharedState>,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<UserQuestionRequestPayload, _> = serde_json::from_value(payload);
    let request = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    if request.agent_id.trim().is_empty() {
        return error_response(req, "agent_id is required");
    }
    if request.questions.is_empty() {
        return error_response(req, "questions is required");
    }

    let project = {
        let agents = shared.agents.lock().await;
        agents
            .agents
            .get(&request.agent_id)
            .map(|rt| rt.record.project.clone())
    };
    let Some(project) = project else {
        return error_response(req, "agent not found");
    };

    let id = generate_request_id();
    let requested_at_ms = now_ms();
    let stored = UserQuestion {
        id: id.clone(),
        agent_id: request.agent_id,
        project,
        questions: request.questions,
        requested_at_ms,
    };

    let (tx, rx) = oneshot::channel::<UserQuestionResponse>();
    {
        let mut pending = shared.pending_questions.lock().await;
        pending.insert(stored.clone(), tx);
    }

    let evt_payload = serde_json::to_value(&stored).unwrap_or(serde_json::Value::Null);
    let evt_id = shared.next_event_id.fetch_add(1, Ordering::Relaxed);
    let _ = shared.events_tx.send(murmur_protocol::Event {
        r#type: EVT_USER_QUESTION.to_owned(),
        id: format!("evt-{evt_id}"),
        payload: evt_payload,
    });

    let response = match rx.await {
        Ok(v) => v,
        Err(_) => return error_response(req, "question request canceled"),
    };

    Response {
        r#type: MSG_QUESTION_REQUEST.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(response).unwrap_or(serde_json::Value::Null),
    }
}

pub(in crate::daemon) async fn handle_question_list(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<UserQuestionListRequest, _> = serde_json::from_value(payload);
    let list = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let mut requests = {
        let pending = shared.pending_questions.lock().await;
        pending.list()
    };
    if let Some(project) = list.project.as_deref() {
        requests.retain(|r| r.project == project);
    }
    requests.sort_by(|a, b| {
        a.requested_at_ms
            .cmp(&b.requested_at_ms)
            .then(a.id.cmp(&b.id))
    });

    let payload = UserQuestionListResponse { requests };
    Response {
        r#type: MSG_QUESTION_LIST.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    }
}

pub(in crate::daemon) async fn handle_question_respond(
    shared: &SharedState,
    mut req: Request,
) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<UserQuestionRespondPayload, _> = serde_json::from_value(payload);
    let respond = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    if respond.id.trim().is_empty() {
        return error_response(req, "id is required");
    }

    let (ok, project) = {
        let mut pending = shared.pending_questions.lock().await;
        // Get the project before responding (respond() removes the item)
        let project = pending.pending.get(&respond.id).map(|q| q.request.project.clone());
        let ok = pending.respond(UserQuestionResponse {
            id: respond.id,
            answers: respond.answers,
        });
        (ok, project)
    };
    if !ok {
        return error_response(req, "question request not found");
    }

    // Record user activity for intervention detection
    if let Some(project) = project {
        shared.record_user_activity(&project).await;
    }

    Response {
        r#type: MSG_QUESTION_RESPOND.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::Value::Null,
    }
}
