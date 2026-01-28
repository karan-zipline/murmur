//! Agent Host Protocol
//!
//! Defines the protocol for communication between the murmur daemon and agent host processes.
//! Agent hosts wrap agent subprocesses and survive daemon restarts.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                           Daemon                                 │
//! │  ┌──────────────────┐                                           │
//! │  │   HostManager    │──────> Unix socket communication          │
//! │  └──────────────────┘                                           │
//! └─────────────────────────────────────────────────────────────────┘
//!               │                            │
//!               ↓                            ↓
//! ┌─────────────────────────┐  ┌─────────────────────────┐
//! │      murmur-host        │  │      murmur-host        │
//! │  ┌─────────────────┐    │  │  ┌─────────────────┐    │
//! │  │   Agent a-1     │    │  │  │   Agent a-2     │    │
//! │  │   (claude)      │    │  │  │   (codex)       │    │
//! │  └─────────────────┘    │  │  └─────────────────┘    │
//! │  Socket: a-1.sock       │  │  Socket: a-2.sock       │
//! └─────────────────────────┘  └─────────────────────────┘
//! ```
//!
//! ## Protocol
//!
//! Communication uses JSONL (newline-delimited JSON) over Unix sockets.
//! Each message is a single JSON object followed by a newline.

use serde::{Deserialize, Serialize};

/// Protocol version for compatibility checking.
pub const HOST_PROTOCOL_VERSION: &str = "1.0";

/// Message types for daemon -> host requests.
pub mod msg {
    pub const PING: &str = "host.ping";
    pub const STATUS: &str = "host.status";
    pub const LIST: &str = "host.list";
    pub const ATTACH: &str = "host.attach";
    pub const DETACH: &str = "host.detach";
    pub const SEND: &str = "host.send";
    pub const STOP: &str = "host.stop";
}

/// Request from daemon to host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostRequest {
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(default)]
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

/// Response from host to daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostResponse {
    #[serde(rename = "type")]
    pub msg_type: String,
    #[serde(default)]
    pub id: String,
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

impl HostResponse {
    /// Create a success response.
    pub fn ok(msg_type: &str, id: &str) -> Self {
        Self {
            msg_type: msg_type.to_owned(),
            id: id.to_owned(),
            success: true,
            error: None,
            payload: None,
        }
    }

    /// Create a success response with payload.
    pub fn ok_with_payload<T: Serialize>(msg_type: &str, id: &str, payload: &T) -> Self {
        Self {
            msg_type: msg_type.to_owned(),
            id: id.to_owned(),
            success: true,
            error: None,
            payload: serde_json::to_value(payload).ok(),
        }
    }

    /// Create an error response.
    pub fn err(msg_type: &str, id: &str, error: impl Into<String>) -> Self {
        Self {
            msg_type: msg_type.to_owned(),
            id: id.to_owned(),
            success: false,
            error: Some(error.into()),
            payload: None,
        }
    }
}

/// Ping response payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PingResponse {
    pub version: String,
    pub uptime_secs: u64,
}

/// Status response payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusResponse {
    pub agent: HostAgentInfo,
    pub stream_offset: i64,
    pub attached_clients: usize,
}

/// Agent information as reported by the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostAgentInfo {
    pub id: String,
    pub project: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    pub worktree: String,
    pub started_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub backend: String,
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codex_thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

/// List response payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListResponse {
    pub agents: Vec<HostAgentInfo>,
}

/// Attach request payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachRequest {
    /// Offset to resume from. Use 0 for all history, -1 for latest only.
    #[serde(default)]
    pub offset: i64,
}

/// Attach response payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachResponse {
    pub current_offset: i64,
}

/// Send request payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendRequest {
    pub input: String,
}

/// Stop request payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopRequest {
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub timeout_secs: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Stop response payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopResponse {
    pub stopped: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

/// Stream event sent to attached clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamEvent {
    /// Event type: "output", "state", "chat"
    #[serde(rename = "type")]
    pub event_type: String,
    pub agent_id: String,
    pub offset: i64,
    pub timestamp: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat: Option<StreamChatEntry>,
}

/// Chat entry in a stream event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamChatEntry {
    pub role: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_input: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_result: Option<String>,
    #[serde(default)]
    pub is_error: bool,
    pub ts_ms: u64,
}

/// Stream event types.
pub mod stream {
    pub const OUTPUT: &str = "output";
    pub const STATE: &str = "state";
    pub const CHAT: &str = "chat";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trip() {
        let req = HostRequest {
            msg_type: msg::PING.to_owned(),
            id: "req-1".to_owned(),
            payload: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: HostRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, req);
    }

    #[test]
    fn response_ok_round_trip() {
        let resp = HostResponse::ok(msg::PING, "req-1");
        let json = serde_json::to_string(&resp).unwrap();
        let back: HostResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.success, true);
        assert_eq!(back.error, None);
    }

    #[test]
    fn response_err_round_trip() {
        let resp = HostResponse::err(msg::PING, "req-1", "something went wrong");
        let json = serde_json::to_string(&resp).unwrap();
        let back: HostResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back.success, false);
        assert_eq!(back.error.as_deref(), Some("something went wrong"));
    }

    #[test]
    fn ping_response_round_trip() {
        let ping = PingResponse {
            version: HOST_PROTOCOL_VERSION.to_owned(),
            uptime_secs: 123,
        };
        let json = serde_json::to_string(&ping).unwrap();
        let back: PingResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ping);
    }

    #[test]
    fn stream_event_round_trip() {
        let event = StreamEvent {
            event_type: stream::CHAT.to_owned(),
            agent_id: "a-1".to_owned(),
            offset: 42,
            timestamp: "2025-01-01T00:00:00Z".to_owned(),
            data: None,
            state: None,
            chat: Some(StreamChatEntry {
                role: "assistant".to_owned(),
                content: "Hello".to_owned(),
                tool_name: None,
                tool_input: None,
                tool_use_id: None,
                tool_result: None,
                is_error: false,
                ts_ms: 1234567890,
            }),
        };
        let json = serde_json::to_string(&event).unwrap();
        let back: StreamEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn attach_request_defaults() {
        let json = r#"{}"#;
        let req: AttachRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.offset, 0);
    }

    #[test]
    fn stop_request_defaults() {
        let json = r#"{}"#;
        let req: StopRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.force, false);
        assert_eq!(req.timeout_secs, 0);
        assert_eq!(req.reason, None);
    }
}
