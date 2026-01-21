use murmur_protocol::{
    Request, Response, StatsRequest, StatsResponse, UsageStats as ProtoUsageStats, MSG_STATS,
};

use super::super::SharedState;
use super::error_response;

pub(in crate::daemon) async fn handle_stats(shared: &SharedState, mut req: Request) -> Response {
    let payload = std::mem::take(&mut req.payload);
    let parsed: Result<StatsRequest, _> = serde_json::from_value(payload);
    let stats = match parsed {
        Ok(v) => v,
        Err(err) => return error_response(req, &format!("invalid payload: {err}")),
    };

    let commit_count = {
        let commits = shared.commits.lock().await;
        commits
            .iter()
            .filter(|(project, _)| match stats.project.as_deref() {
                None => true,
                Some(filter) => filter == project.as_str(),
            })
            .map(|(_, log)| log.len() as u32)
            .sum::<u32>()
    };

    let usage = match crate::stats::get_current_billing_window_with_usage() {
        Ok(window) => {
            let limits = murmur_core::usage::default_pro_limits();
            let percent = window.usage.percent_int(limits);
            let time_left = crate::stats::format_duration(window.window.time_remaining(window.now));
            let window_end = crate::stats::format_rfc3339(window.window.end).unwrap_or_default();

            ProtoUsageStats {
                output_tokens: window.usage.output_tokens,
                percent,
                window_end,
                time_left,
                plan_limit: limits.output_tokens,
                plan: "pro".to_owned(),
            }
        }
        Err(err) => {
            tracing::debug!(error = %err, "failed to get usage stats");
            ProtoUsageStats {
                output_tokens: 0,
                percent: 0,
                window_end: String::new(),
                time_left: String::new(),
                plan_limit: 0,
                plan: "pro".to_owned(),
            }
        }
    };

    let payload = StatsResponse {
        commit_count,
        usage,
    };

    Response {
        r#type: MSG_STATS.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(payload).unwrap_or(serde_json::Value::Null),
    }
}
