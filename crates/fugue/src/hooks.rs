use std::collections::BTreeMap;
use std::env;
use std::io::Read as _;

use anyhow::{anyhow, Context as _};
use directories::BaseDirs;
use fugue_core::paths::FuguePaths;
use fugue_core::permissions::{evaluate_rules, Action};
use fugue_protocol::{
    PermissionBehavior, PermissionRequestPayload, QuestionItem, UserQuestionRequestPayload,
};
use serde::{Deserialize, Serialize};

use crate::{client, permissions};

#[derive(Debug, Deserialize)]
struct HookInput {
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub tool_name: String,
    #[serde(default)]
    pub tool_input: serde_json::Value,
    #[serde(default)]
    pub tool_use_id: String,
}

#[derive(Debug, Serialize)]
struct PreToolUseOutput {
    #[serde(rename = "hookSpecificOutput")]
    pub hook_specific_output: PreToolUseSpecificOutput,
}

#[derive(Debug, Serialize)]
struct PreToolUseSpecificOutput {
    #[serde(rename = "hookEventName")]
    pub hook_event_name: String,
    #[serde(rename = "permissionDecision")]
    pub permission_decision: String,
    #[serde(
        rename = "permissionDecisionReason",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub permission_decision_reason: Option<String>,
    #[serde(
        rename = "updatedInput",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub updated_input: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
struct AskUserQuestionInput {
    pub questions: Vec<QuestionItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answers: Option<BTreeMap<String, String>>,
}

pub async fn handle_pre_tool_use(paths: &FuguePaths) -> anyhow::Result<()> {
    let hook_input = read_hook_input()?;

    if hook_input.tool_name == "AskUserQuestion" {
        return handle_ask_user_question(paths, hook_input).await;
    }

    let home_dir = BaseDirs::new()
        .ok_or_else(|| anyhow!("could not determine home directory"))?
        .home_dir()
        .to_string_lossy()
        .to_string();

    let project = env::var("FUGUE_PROJECT")
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty());
    let rules = permissions::load_rules(paths, project.as_deref()).await?;

    let (action, matched) = evaluate_rules(
        &rules,
        &hook_input.tool_name,
        &hook_input.tool_input,
        &hook_input.cwd,
        &home_dir,
    );

    if matched {
        match action {
            Action::Allow => return output_pre_tool_use("allow", None, None),
            Action::Deny => {
                return output_pre_tool_use("deny", Some("blocked by permission rule"), None);
            }
            Action::Pass => {}
        }
    }

    let agent_id = agent_id_from_env()
        .ok_or_else(|| anyhow!("FUGUE_AGENT_ID is not set (not running as a managed agent)"))?;

    let tool_use_id = hook_input.tool_use_id.trim().to_owned();
    let tool_use_id = if tool_use_id.is_empty() {
        None
    } else {
        Some(tool_use_id)
    };

    let resp = match client::permission_request(
        paths,
        PermissionRequestPayload {
            agent_id,
            tool_name: hook_input.tool_name,
            tool_input: hook_input.tool_input,
            tool_use_id,
        },
    )
    .await
    {
        Ok(v) => v,
        Err(err) => {
            let reason = format!("permission request failed: {err:#}");
            return output_pre_tool_use("deny", Some(&reason), None);
        }
    };

    match resp.behavior {
        PermissionBehavior::Allow => output_pre_tool_use("allow", None, None),
        PermissionBehavior::Deny => output_pre_tool_use("deny", resp.message.as_deref(), None),
    }
}

pub async fn handle_stop(paths: &FuguePaths) -> anyhow::Result<()> {
    let Some(agent_id) = agent_id_from_env() else {
        return Ok(());
    };

    let _ = client::agent_idle(paths, agent_id).await;
    Ok(())
}

fn agent_id_from_env() -> Option<String> {
    env::var("FUGUE_AGENT_ID")
        .or_else(|_| env::var("FAB_AGENT_ID"))
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
}

fn read_hook_input() -> anyhow::Result<HookInput> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("read stdin")?;
    serde_json::from_str(&buf).context("parse hook input JSON")
}

fn output_pre_tool_use(
    decision: &str,
    reason: Option<&str>,
    updated_input: Option<serde_json::Value>,
) -> anyhow::Result<()> {
    let output = PreToolUseOutput {
        hook_specific_output: PreToolUseSpecificOutput {
            hook_event_name: "PreToolUse".to_owned(),
            permission_decision: decision.to_owned(),
            permission_decision_reason: reason
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            updated_input,
        },
    };

    let json = serde_json::to_string(&output).context("serialize hook output")?;
    println!("{json}");
    Ok(())
}

async fn handle_ask_user_question(paths: &FuguePaths, hook_input: HookInput) -> anyhow::Result<()> {
    let mut ask: AskUserQuestionInput = serde_json::from_value(hook_input.tool_input)
        .context("parse AskUserQuestion tool_input")?;

    if ask.questions.is_empty() {
        return output_pre_tool_use("deny", Some("no questions provided"), None);
    }

    let agent_id = agent_id_from_env()
        .ok_or_else(|| anyhow!("FUGUE_AGENT_ID is not set (not running as a managed agent)"))?;

    let resp = match client::question_request(
        paths,
        UserQuestionRequestPayload {
            agent_id,
            questions: ask.questions.clone(),
        },
    )
    .await
    {
        Ok(v) => v,
        Err(err) => {
            let reason = format!("user question failed: {err:#}");
            return output_pre_tool_use("deny", Some(&reason), None);
        }
    };

    ask.answers = Some(resp.answers);
    let updated_input =
        serde_json::to_value(ask).context("serialize AskUserQuestion updatedInput")?;

    output_pre_tool_use("allow", None, Some(updated_input))
}
