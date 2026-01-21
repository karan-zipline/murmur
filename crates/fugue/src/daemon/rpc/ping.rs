use std::time::UNIX_EPOCH;

use anyhow::anyhow;
use fugue_protocol::{PingResponse, Request, Response, MSG_PING, PROTOCOL_VERSION};

use super::super::SharedState;

pub(in crate::daemon) fn handle_ping(
    shared: &SharedState,
    req: Request,
) -> anyhow::Result<Response> {
    if req.r#type != MSG_PING {
        return Err(anyhow!("unexpected request type"));
    }

    let started_at_ms = shared
        .started_at
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let uptime_ms = shared.started_at_instant.elapsed().as_millis() as u64;

    let payload = PingResponse {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        protocol: PROTOCOL_VERSION.to_owned(),
        pid: shared.pid,
        started_at_ms,
        uptime_ms,
    };

    Ok(Response {
        r#type: MSG_PING.to_owned(),
        id: req.id,
        success: true,
        error: None,
        payload: serde_json::to_value(payload)?,
    })
}
