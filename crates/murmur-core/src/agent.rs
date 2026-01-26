use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    Coding,
    Planner,
    Manager,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentState {
    Starting,
    Running,
    Idle,
    NeedsResolution,
    Exited,
    Aborted,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentExitReason {
    Exited,
    Aborted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRecord {
    pub id: String,
    pub project: String,
    pub role: AgentRole,
    pub issue_id: String,
    pub state: AgentState,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub worktree_dir: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_reason: Option<AgentExitReason>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum AgentEvent<'a> {
    Spawned { pid: u32 },
    NeedsResolution { reason: &'a str },
    AssignedIssue { issue_id: &'a str },
    Described { description: &'a str },
    Exited { code: Option<i32> },
    Aborted { by: &'a str },
    BecameIdle,
    ResumedFromIdle,
}

impl AgentRecord {
    pub fn new(
        id: String,
        project: String,
        role: AgentRole,
        issue_id: String,
        created_at_ms: u64,
        worktree_dir: String,
    ) -> Self {
        Self {
            id,
            project,
            role,
            issue_id,
            state: AgentState::Starting,
            created_at_ms,
            updated_at_ms: created_at_ms,
            worktree_dir,
            description: None,
            pid: None,
            exit_code: None,
            exit_reason: None,
        }
    }

    pub fn apply_event(&self, event: AgentEvent<'_>, now_ms: u64) -> Self {
        let mut next = self.clone();
        next.updated_at_ms = now_ms;

        match event {
            AgentEvent::Spawned { pid } => {
                next.pid = Some(pid);
                next.state = AgentState::Running;
            }
            AgentEvent::NeedsResolution { .. } => {
                next.state = AgentState::NeedsResolution;
            }
            AgentEvent::AssignedIssue { issue_id } => {
                let trimmed = issue_id.trim();
                if !trimmed.is_empty() {
                    next.issue_id = trimmed.to_owned();
                }
            }
            AgentEvent::Described { description } => {
                let trimmed = description.trim();
                next.description = if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_owned())
                };
            }
            AgentEvent::Exited { code } => {
                next.pid = None;
                next.exit_code = code;
                next.exit_reason = Some(AgentExitReason::Exited);
                next.state = AgentState::Exited;
            }
            AgentEvent::Aborted { .. } => {
                next.pid = None;
                next.exit_reason = Some(AgentExitReason::Aborted);
                next.state = AgentState::Aborted;
            }
            AgentEvent::BecameIdle => {
                next.state = AgentState::Idle;
            }
            AgentEvent::ResumedFromIdle => {
                next.state = AgentState::Running;
            }
        }

        next
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatRole {
    User,
    Assistant,
    Tool,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
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

impl ChatMessage {
    pub fn new(role: ChatRole, content: impl Into<String>, ts_ms: u64) -> Self {
        Self {
            role,
            content: content.into(),
            tool_name: None,
            tool_input: None,
            tool_use_id: None,
            tool_result: None,
            is_error: false,
            ts_ms,
        }
    }

    pub fn tool_use(
        tool_name: impl Into<String>,
        tool_input: impl Into<String>,
        tool_use_id: Option<String>,
        ts_ms: u64,
    ) -> Self {
        let tool_name = tool_name.into();
        let tool_input = tool_input.into();
        let content = if tool_input.trim().is_empty() {
            tool_name.clone()
        } else {
            format!("{tool_name}: {tool_input}")
        };

        Self {
            role: ChatRole::Tool,
            content,
            tool_name: Some(tool_name),
            tool_input: (!tool_input.trim().is_empty()).then_some(tool_input),
            tool_use_id,
            tool_result: None,
            is_error: false,
            ts_ms,
        }
    }

    pub fn tool_result(
        tool_result: impl Into<String>,
        tool_use_id: Option<String>,
        is_error: bool,
        ts_ms: u64,
    ) -> Self {
        let tool_result = tool_result.into();
        Self {
            role: ChatRole::Tool,
            content: tool_result.clone(),
            tool_name: None,
            tool_input: None,
            tool_use_id,
            tool_result: Some(tool_result),
            is_error,
            ts_ms,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChatHistory {
    capacity: usize,
    messages: VecDeque<ChatMessage>,
}

impl ChatHistory {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            messages: VecDeque::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    pub fn push(&mut self, msg: ChatMessage) {
        if self.messages.len() == self.capacity {
            self.messages.pop_front();
        }
        self.messages.push_back(msg);
    }

    pub fn tail(&self, limit: usize) -> Vec<ChatMessage> {
        let limit = limit.min(self.messages.len());
        self.messages
            .iter()
            .skip(self.messages.len() - limit)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_transitions_start_to_running() {
        let a = AgentRecord::new(
            "a-1".to_owned(),
            "demo".to_owned(),
            AgentRole::Coding,
            "ISSUE-1".to_owned(),
            1000,
            "/tmp/wt".to_owned(),
        );
        assert_eq!(a.state, AgentState::Starting);

        let b = a.apply_event(AgentEvent::Spawned { pid: 123 }, 1100);
        assert_eq!(b.state, AgentState::Running);
        assert_eq!(b.pid, Some(123));
        assert_eq!(b.updated_at_ms, 1100);
    }

    #[test]
    fn agent_transitions_running_to_aborted() {
        let a = AgentRecord::new(
            "a-1".to_owned(),
            "demo".to_owned(),
            AgentRole::Coding,
            "ISSUE-1".to_owned(),
            1000,
            "/tmp/wt".to_owned(),
        )
        .apply_event(AgentEvent::Spawned { pid: 123 }, 1100);

        let b = a.apply_event(AgentEvent::Aborted { by: "test" }, 1200);
        assert_eq!(b.state, AgentState::Aborted);
        assert_eq!(b.pid, None);
        assert_eq!(b.exit_reason, Some(AgentExitReason::Aborted));
        assert_eq!(b.updated_at_ms, 1200);
    }

    #[test]
    fn chat_history_trims_to_capacity() {
        let mut h = ChatHistory::new(2);
        h.push(ChatMessage::new(ChatRole::User, "1", 1));
        h.push(ChatMessage::new(ChatRole::Assistant, "2", 2));
        h.push(ChatMessage::new(ChatRole::Assistant, "3", 3));

        let got = h.tail(10);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].content, "2");
        assert_eq!(got[1].content, "3");
    }

    #[test]
    fn agent_transitions_running_to_idle() {
        let a = AgentRecord::new(
            "a-1".to_owned(),
            "demo".to_owned(),
            AgentRole::Coding,
            "ISSUE-1".to_owned(),
            1000,
            "/tmp/wt".to_owned(),
        )
        .apply_event(AgentEvent::Spawned { pid: 123 }, 1100);
        assert_eq!(a.state, AgentState::Running);

        let b = a.apply_event(AgentEvent::BecameIdle, 1200);
        assert_eq!(b.state, AgentState::Idle);
        assert_eq!(b.pid, Some(123)); // pid preserved when idle
        assert_eq!(b.updated_at_ms, 1200);
    }

    #[test]
    fn agent_transitions_idle_to_running() {
        let a = AgentRecord::new(
            "a-1".to_owned(),
            "demo".to_owned(),
            AgentRole::Coding,
            "ISSUE-1".to_owned(),
            1000,
            "/tmp/wt".to_owned(),
        )
        .apply_event(AgentEvent::Spawned { pid: 123 }, 1100)
        .apply_event(AgentEvent::BecameIdle, 1200);
        assert_eq!(a.state, AgentState::Idle);

        let b = a.apply_event(AgentEvent::ResumedFromIdle, 1300);
        assert_eq!(b.state, AgentState::Running);
        assert_eq!(b.pid, Some(123)); // pid preserved
        assert_eq!(b.updated_at_ms, 1300);
    }

    #[test]
    fn agent_transitions_idle_to_exited() {
        let a = AgentRecord::new(
            "a-1".to_owned(),
            "demo".to_owned(),
            AgentRole::Coding,
            "ISSUE-1".to_owned(),
            1000,
            "/tmp/wt".to_owned(),
        )
        .apply_event(AgentEvent::Spawned { pid: 123 }, 1100)
        .apply_event(AgentEvent::BecameIdle, 1200);
        assert_eq!(a.state, AgentState::Idle);

        let b = a.apply_event(AgentEvent::Exited { code: Some(0) }, 1300);
        assert_eq!(b.state, AgentState::Exited);
        assert_eq!(b.pid, None);
        assert_eq!(b.exit_code, Some(0));
        assert_eq!(b.exit_reason, Some(AgentExitReason::Exited));
        assert_eq!(b.updated_at_ms, 1300);
    }

    #[test]
    fn agent_transitions_idle_to_aborted() {
        let a = AgentRecord::new(
            "a-1".to_owned(),
            "demo".to_owned(),
            AgentRole::Coding,
            "ISSUE-1".to_owned(),
            1000,
            "/tmp/wt".to_owned(),
        )
        .apply_event(AgentEvent::Spawned { pid: 123 }, 1100)
        .apply_event(AgentEvent::BecameIdle, 1200);
        assert_eq!(a.state, AgentState::Idle);

        let b = a.apply_event(AgentEvent::Aborted { by: "user" }, 1300);
        assert_eq!(b.state, AgentState::Aborted);
        assert_eq!(b.pid, None);
        assert_eq!(b.exit_reason, Some(AgentExitReason::Aborted));
        assert_eq!(b.updated_at_ms, 1300);
    }

    #[test]
    fn agent_state_idle_serialization_roundtrip() {
        let a = AgentRecord::new(
            "a-1".to_owned(),
            "demo".to_owned(),
            AgentRole::Coding,
            "ISSUE-1".to_owned(),
            1000,
            "/tmp/wt".to_owned(),
        )
        .apply_event(AgentEvent::Spawned { pid: 123 }, 1100)
        .apply_event(AgentEvent::BecameIdle, 1200);
        assert_eq!(a.state, AgentState::Idle);

        let json = serde_json::to_string(&a).unwrap();
        assert!(json.contains("\"idle\""));

        let b: AgentRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(b.state, AgentState::Idle);
        assert_eq!(b.id, a.id);
        assert_eq!(b.pid, a.pid);
    }
}
