use murmur_protocol::{Request, Response};

mod agent;
mod claim;
mod commit;
mod director;
mod issue;
mod manager;
mod orchestration;
mod permission;
mod ping;
mod plan;
mod project;
mod question;
mod stats;

pub(super) use agent::{
    handle_agent_abort, handle_agent_chat_history, handle_agent_claim, handle_agent_create,
    handle_agent_delete, handle_agent_describe, handle_agent_done, handle_agent_idle,
    handle_agent_list, handle_agent_send_message, handle_agent_sync_comments,
};
pub(super) use claim::handle_claim_list;
pub(super) use commit::handle_commit_list;
pub(super) use issue::{
    handle_issue_close, handle_issue_comment, handle_issue_commit, handle_issue_create,
    handle_issue_get, handle_issue_list, handle_issue_list_comments, handle_issue_plan,
    handle_issue_ready, handle_issue_update,
};
pub(super) use director::{
    handle_director_chat_history, handle_director_clear_history, handle_director_send_message,
    handle_director_start, handle_director_status, handle_director_stop,
};
pub(super) use manager::{
    handle_manager_chat_history, handle_manager_clear_history, handle_manager_send_message,
    handle_manager_start, handle_manager_status, handle_manager_stop,
};
pub(super) use orchestration::{
    handle_orchestration_start, handle_orchestration_status, handle_orchestration_stop,
};
pub(super) use permission::{
    handle_permission_list, handle_permission_request, handle_permission_respond,
};
pub(super) use ping::handle_ping;
pub(super) use plan::{
    handle_plan_chat_history, handle_plan_list, handle_plan_send_message, handle_plan_show,
    handle_plan_start, handle_plan_stop,
};
pub(super) use project::{
    handle_project_add, handle_project_config_get, handle_project_config_set,
    handle_project_config_show, handle_project_list, handle_project_remove, handle_project_status,
};
pub(super) use question::{handle_question_list, handle_question_request, handle_question_respond};
pub(super) use stats::handle_stats;

pub(super) fn error_response(req: Request, msg: &str) -> Response {
    Response {
        r#type: req.r#type,
        id: req.id,
        success: false,
        error: Some(msg.to_owned()),
        payload: serde_json::Value::Null,
    }
}
