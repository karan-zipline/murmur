use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::paths::safe_join;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConfigFile {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<ProjectConfig>,

    #[serde(default, rename = "log_level", skip_serializing_if = "Option::is_none")]
    pub log_level: Option<String>,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub providers: BTreeMap<String, toml::Value>,

    #[serde(
        default,
        rename = "llm_auth",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub llm_auth: BTreeMap<String, toml::Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhook: Option<WebhookConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub polling: Option<PollingConfig>,

    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WebhookConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(rename = "bind-addr", alias = "bind_addr", default)]
    pub bind_addr: String,

    #[serde(default)]
    pub secret: String,

    #[serde(rename = "path-prefix", alias = "path_prefix", default)]
    pub path_prefix: String,

    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, toml::Value>,
}

pub const DEFAULT_WEBHOOK_BIND_ADDR: &str = ":8080";
pub const DEFAULT_WEBHOOK_PATH_PREFIX: &str = "/webhooks";

impl WebhookConfig {
    pub fn effective_bind_addr(&self) -> &str {
        let trimmed = self.bind_addr.trim();
        if trimmed.is_empty() {
            DEFAULT_WEBHOOK_BIND_ADDR
        } else {
            trimmed
        }
    }

    pub fn effective_path_prefix(&self) -> &str {
        let trimmed = self.path_prefix.trim();
        if trimmed.is_empty() {
            DEFAULT_WEBHOOK_PATH_PREFIX
        } else {
            trimmed
        }
    }
}

/// Configuration for background polling tasks.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PollingConfig {
    /// Enable/disable comment polling (default: true)
    #[serde(
        default = "PollingConfig::default_comment_polling_enabled",
        rename = "comment-polling-enabled",
        alias = "comment_polling_enabled"
    )]
    pub comment_polling_enabled: bool,

    /// Comment poll interval in seconds (default: 10)
    #[serde(
        default = "PollingConfig::default_comment_interval_secs",
        rename = "comment-interval-secs",
        alias = "comment_interval_secs"
    )]
    pub comment_interval_secs: u64,
}

pub const DEFAULT_COMMENT_POLL_INTERVAL_SECS: u64 = 10;

impl PollingConfig {
    fn default_comment_polling_enabled() -> bool {
        true
    }

    fn default_comment_interval_secs() -> u64 {
        DEFAULT_COMMENT_POLL_INTERVAL_SECS
    }

    pub fn effective_comment_polling_enabled(&self) -> bool {
        self.comment_polling_enabled
    }

    pub fn effective_comment_interval_secs(&self) -> u64 {
        if self.comment_interval_secs == 0 {
            DEFAULT_COMMENT_POLL_INTERVAL_SECS
        } else {
            self.comment_interval_secs
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub name: String,

    #[serde(rename = "remote-url", alias = "remote_url")]
    pub remote_url: String,

    #[serde(
        rename = "max-agents",
        alias = "max_agents",
        default = "default_max_agents"
    )]
    pub max_agents: u16,

    #[serde(rename = "issue-backend", alias = "issue_backend", default)]
    pub issue_backend: IssueBackend,

    #[serde(rename = "permissions-checker", alias = "permissions_checker", default)]
    pub permissions_checker: PermissionsChecker,

    #[serde(rename = "agent-backend", alias = "agent_backend", default)]
    pub agent_backend: AgentBackend,

    #[serde(rename = "planner-backend", alias = "planner_backend", default)]
    pub planner_backend: Option<AgentBackend>,

    #[serde(rename = "coding-backend", alias = "coding_backend", default)]
    pub coding_backend: Option<AgentBackend>,

    #[serde(rename = "merge-strategy", alias = "merge_strategy", default)]
    pub merge_strategy: MergeStrategy,

    #[serde(rename = "allowed-authors", alias = "allowed_authors", default)]
    pub allowed_authors: Vec<String>,

    #[serde(default)]
    pub autostart: bool,

    #[serde(rename = "linear-team", default)]
    pub linear_team: Option<String>,

    #[serde(rename = "linear-project", default)]
    pub linear_project: Option<String>,

    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, toml::Value>,
}

impl ProjectConfig {
    pub fn effective_planner_backend(&self) -> AgentBackend {
        self.planner_backend.unwrap_or(self.agent_backend)
    }

    pub fn effective_coding_backend(&self) -> AgentBackend {
        self.coding_backend.unwrap_or(self.agent_backend)
    }
}

fn default_max_agents() -> u16 {
    3
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum IssueBackend {
    #[serde(rename = "tk")]
    #[default]
    Tk,
    #[serde(rename = "github")]
    Github,
    #[serde(rename = "gh")]
    Gh,
    #[serde(rename = "linear")]
    Linear,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PermissionsChecker {
    #[serde(rename = "manual")]
    #[default]
    Manual,
    #[serde(rename = "llm")]
    Llm,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AgentBackend {
    #[serde(rename = "claude")]
    Claude,
    #[serde(rename = "codex")]
    #[default]
    Codex,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MergeStrategy {
    #[serde(rename = "direct")]
    #[default]
    Direct,
    #[serde(rename = "pull-request")]
    PullRequest,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("project name is empty")]
    ProjectNameEmpty,
    #[error("invalid project name: {name}")]
    InvalidProjectName { name: String },
    #[error("project already exists: {name}")]
    ProjectAlreadyExists { name: String },
    #[error("project not found: {name}")]
    ProjectNotFound { name: String },
    #[error("remote url is empty")]
    RemoteUrlEmpty,
    #[error("max-agents must be > 0")]
    InvalidMaxAgents,
    #[error("unknown config key: {key}")]
    UnknownKey { key: String },
    #[error("invalid value for {key}: {value}")]
    InvalidValue { key: String, value: String },
    #[error("linear backend requires `linear-team`")]
    LinearTeamMissing,
}

impl ConfigFile {
    pub fn validate(&self) -> Result<(), ConfigError> {
        let mut names = BTreeSet::new();
        for p in &self.projects {
            validate_project_name(&p.name)?;
            if !names.insert(p.name.clone()) {
                return Err(ConfigError::ProjectAlreadyExists {
                    name: p.name.clone(),
                });
            }

            if p.remote_url.trim().is_empty() {
                return Err(ConfigError::RemoteUrlEmpty);
            }

            if p.max_agents == 0 {
                return Err(ConfigError::InvalidMaxAgents);
            }

            if p.issue_backend == IssueBackend::Linear {
                let has_team = p.linear_team.as_ref().is_some_and(|s| !s.trim().is_empty());
                if !has_team {
                    return Err(ConfigError::LinearTeamMissing);
                }
            }
        }

        Ok(())
    }

    pub fn project(&self, name: &str) -> Option<&ProjectConfig> {
        self.projects.iter().find(|p| p.name == name)
    }

    /// Returns the effective polling config, using defaults if not specified.
    pub fn effective_polling(&self) -> PollingConfig {
        self.polling.clone().unwrap_or_default()
    }

    pub fn add_project(&self, project: ProjectConfig) -> Result<Self, ConfigError> {
        validate_project_name(&project.name)?;
        if self.project(&project.name).is_some() {
            return Err(ConfigError::ProjectAlreadyExists { name: project.name });
        }
        if project.remote_url.trim().is_empty() {
            return Err(ConfigError::RemoteUrlEmpty);
        }
        if project.max_agents == 0 {
            return Err(ConfigError::InvalidMaxAgents);
        }

        let mut next = self.clone();
        next.projects.push(project);
        next.validate()?;
        Ok(next)
    }

    pub fn remove_project(&self, name: &str) -> Result<Self, ConfigError> {
        if self.project(name).is_none() {
            return Err(ConfigError::ProjectNotFound {
                name: name.to_owned(),
            });
        }

        let mut next = self.clone();
        next.projects.retain(|p| p.name != name);
        Ok(next)
    }

    pub fn set_project_key(&self, name: &str, key: &str, value: &str) -> Result<Self, ConfigError> {
        let key = normalize_key(key);

        let Some(existing) = self.project(name) else {
            return Err(ConfigError::ProjectNotFound {
                name: name.to_owned(),
            });
        };

        let mut updated = existing.clone();

        match key.as_str() {
            "remote-url" => {
                if value.trim().is_empty() {
                    return Err(ConfigError::RemoteUrlEmpty);
                }
                updated.remote_url = value.to_owned();
            }
            "max-agents" => {
                let parsed: u16 = value.parse().map_err(|_| ConfigError::InvalidValue {
                    key: key.clone(),
                    value: value.to_owned(),
                })?;
                if parsed == 0 {
                    return Err(ConfigError::InvalidMaxAgents);
                }
                updated.max_agents = parsed;
            }
            "autostart" => {
                let parsed: bool = value.parse().map_err(|_| ConfigError::InvalidValue {
                    key: key.clone(),
                    value: value.to_owned(),
                })?;
                updated.autostart = parsed;
            }
            "issue-backend" => {
                updated.issue_backend = parse_enum::<IssueBackend>(&key, value)?;
            }
            "permissions-checker" => {
                updated.permissions_checker = parse_enum::<PermissionsChecker>(&key, value)?;
            }
            "agent-backend" => {
                updated.agent_backend = parse_enum::<AgentBackend>(&key, value)?;
            }
            "planner-backend" => {
                updated.planner_backend = Some(parse_enum::<AgentBackend>(&key, value)?);
            }
            "coding-backend" => {
                updated.coding_backend = Some(parse_enum::<AgentBackend>(&key, value)?);
            }
            "merge-strategy" => {
                updated.merge_strategy = parse_enum::<MergeStrategy>(&key, value)?;
            }
            "allowed-authors" => {
                let authors = value
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_owned())
                    .collect::<Vec<_>>();
                updated.allowed_authors = authors;
            }
            "linear-team" => {
                updated.linear_team = (!value.trim().is_empty()).then(|| value.to_owned());
            }
            "linear-project" => {
                updated.linear_project = (!value.trim().is_empty()).then(|| value.to_owned());
            }
            _ => {
                return Err(ConfigError::UnknownKey { key });
            }
        }

        let mut next = self.clone();
        if let Some(p) = next.projects.iter_mut().find(|p| p.name == name) {
            *p = updated;
        }
        next.validate()?;
        Ok(next)
    }

    pub fn get_project_key_value(&self, name: &str, key: &str) -> Result<toml::Value, ConfigError> {
        let key = normalize_key(key);
        let Some(project) = self.project(name) else {
            return Err(ConfigError::ProjectNotFound {
                name: name.to_owned(),
            });
        };

        let value = match key.as_str() {
            "name" => toml::Value::String(project.name.clone()),
            "remote-url" => toml::Value::String(project.remote_url.clone()),
            "max-agents" => toml::Value::Integer(project.max_agents as i64),
            "autostart" => toml::Value::Boolean(project.autostart),
            "issue-backend" => toml::Value::String(format_enum(project.issue_backend)),
            "permissions-checker" => toml::Value::String(format_enum(project.permissions_checker)),
            "agent-backend" => toml::Value::String(format_enum(project.agent_backend)),
            "planner-backend" => {
                toml::Value::String(format_enum(project.effective_planner_backend()))
            }
            "coding-backend" => {
                toml::Value::String(format_enum(project.effective_coding_backend()))
            }
            "merge-strategy" => toml::Value::String(format_enum(project.merge_strategy)),
            "allowed-authors" => toml::Value::Array(
                project
                    .allowed_authors
                    .iter()
                    .map(|s| toml::Value::String(s.clone()))
                    .collect(),
            ),
            "linear-team" => toml::Value::String(project.linear_team.clone().unwrap_or_default()),
            "linear-project" => {
                toml::Value::String(project.linear_project.clone().unwrap_or_default())
            }
            _ => return Err(ConfigError::UnknownKey { key }),
        };

        Ok(value)
    }

    pub fn project_config_map(
        &self,
        name: &str,
    ) -> Result<BTreeMap<String, toml::Value>, ConfigError> {
        let Some(project) = self.project(name) else {
            return Err(ConfigError::ProjectNotFound {
                name: name.to_owned(),
            });
        };

        Ok(project_config_map(project))
    }
}

pub fn project_config_map(project: &ProjectConfig) -> BTreeMap<String, toml::Value> {
    BTreeMap::from([
        (
            "max-agents".to_owned(),
            toml::Value::Integer(project.max_agents as i64),
        ),
        (
            "autostart".to_owned(),
            toml::Value::Boolean(project.autostart),
        ),
        (
            "issue-backend".to_owned(),
            toml::Value::String(format_enum(project.issue_backend)),
        ),
        (
            "permissions-checker".to_owned(),
            toml::Value::String(format_enum(project.permissions_checker)),
        ),
        (
            "agent-backend".to_owned(),
            toml::Value::String(format_enum(project.agent_backend)),
        ),
        (
            "planner-backend".to_owned(),
            toml::Value::String(format_enum(project.effective_planner_backend())),
        ),
        (
            "coding-backend".to_owned(),
            toml::Value::String(format_enum(project.effective_coding_backend())),
        ),
        (
            "merge-strategy".to_owned(),
            toml::Value::String(format_enum(project.merge_strategy)),
        ),
        (
            "allowed-authors".to_owned(),
            toml::Value::Array(
                project
                    .allowed_authors
                    .iter()
                    .map(|s| toml::Value::String(s.clone()))
                    .collect(),
            ),
        ),
        (
            "linear-team".to_owned(),
            toml::Value::String(project.linear_team.clone().unwrap_or_default()),
        ),
        (
            "linear-project".to_owned(),
            toml::Value::String(project.linear_project.clone().unwrap_or_default()),
        ),
    ])
}

fn validate_project_name(name: &str) -> Result<(), ConfigError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ConfigError::ProjectNameEmpty);
    }

    if !trimmed
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(ConfigError::InvalidProjectName {
            name: trimmed.to_owned(),
        });
    }

    safe_join(std::path::Path::new("projects"), trimmed).map_err(|_| {
        ConfigError::InvalidProjectName {
            name: trimmed.to_owned(),
        }
    })?;

    Ok(())
}

fn normalize_key(key: &str) -> String {
    key.trim().replace('_', "-").to_ascii_lowercase()
}

fn parse_enum<T>(key: &str, value: &str) -> Result<T, ConfigError>
where
    T: for<'de> Deserialize<'de>,
{
    let value = value.trim();
    let as_toml = format!("v = \"{value}\"");
    let parsed: toml::Value = toml::from_str(&as_toml).map_err(|_| ConfigError::InvalidValue {
        key: key.to_owned(),
        value: value.to_owned(),
    })?;
    let Some(v) = parsed.get("v") else {
        return Err(ConfigError::InvalidValue {
            key: key.to_owned(),
            value: value.to_owned(),
        });
    };
    v.clone().try_into().map_err(|_| ConfigError::InvalidValue {
        key: key.to_owned(),
        value: value.to_owned(),
    })
}

fn format_enum<T>(value: T) -> String
where
    T: Serialize,
{
    let v = toml::Value::try_from(value).unwrap_or(toml::Value::String("unknown".to_owned()));
    v.as_str().unwrap_or("unknown").to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_duplicate_project_names() {
        let cfg = ConfigFile {
            projects: vec![
                ProjectConfig {
                    name: "demo".to_owned(),
                    remote_url: "file:///tmp/demo.git".to_owned(),
                    max_agents: 3,
                    issue_backend: IssueBackend::Tk,
                    permissions_checker: PermissionsChecker::Manual,
                    agent_backend: AgentBackend::Codex,
                    planner_backend: None,
                    coding_backend: None,
                    merge_strategy: MergeStrategy::Direct,
                    allowed_authors: vec![],
                    autostart: false,
                    linear_team: None,
                    linear_project: None,
                    extra: BTreeMap::new(),
                },
                ProjectConfig {
                    name: "demo".to_owned(),
                    remote_url: "file:///tmp/demo2.git".to_owned(),
                    ..default_project("demo2")
                },
            ],
            ..Default::default()
        };

        let err = cfg.validate().unwrap_err();
        assert!(matches!(err, ConfigError::ProjectAlreadyExists { .. }));
    }

    #[test]
    fn validate_rejects_invalid_name_chars() {
        let cfg = ConfigFile {
            projects: vec![ProjectConfig {
                name: "bad name".to_owned(),
                remote_url: "file:///tmp/demo.git".to_owned(),
                ..default_project("bad name")
            }],
            ..Default::default()
        };

        let err = cfg.validate().unwrap_err();
        assert!(matches!(err, ConfigError::InvalidProjectName { .. }));
    }

    #[test]
    fn validate_rejects_linear_without_team() {
        let cfg = ConfigFile {
            projects: vec![ProjectConfig {
                name: "linear".to_owned(),
                remote_url: "file:///tmp/demo.git".to_owned(),
                issue_backend: IssueBackend::Linear,
                linear_team: None,
                ..default_project("linear")
            }],
            ..Default::default()
        };

        let err = cfg.validate().unwrap_err();
        assert!(matches!(err, ConfigError::LinearTeamMissing));
    }

    #[test]
    fn set_get_project_key_round_trip() {
        let cfg = ConfigFile::default()
            .add_project(default_project("demo"))
            .unwrap();

        let cfg = cfg
            .set_project_key("demo", "max-agents", "5")
            .unwrap()
            .set_project_key("demo", "autostart", "true")
            .unwrap()
            .set_project_key("demo", "issue-backend", "github")
            .unwrap()
            .set_project_key("demo", "allowed-authors", "alice,bob")
            .unwrap();

        assert_eq!(
            cfg.get_project_key_value("demo", "max-agents").unwrap(),
            toml::Value::Integer(5)
        );
        assert_eq!(
            cfg.get_project_key_value("demo", "autostart").unwrap(),
            toml::Value::Boolean(true)
        );
        assert_eq!(
            cfg.get_project_key_value("demo", "issue-backend").unwrap(),
            toml::Value::String("github".to_owned())
        );
        assert_eq!(
            cfg.get_project_key_value("demo", "allowed-authors")
                .unwrap(),
            toml::Value::Array(vec![
                toml::Value::String("alice".to_owned()),
                toml::Value::String("bob".to_owned()),
            ])
        );
    }

    fn default_project(name: &str) -> ProjectConfig {
        ProjectConfig {
            name: name.to_owned(),
            remote_url: "file:///tmp/demo.git".to_owned(),
            max_agents: 3,
            issue_backend: IssueBackend::Tk,
            permissions_checker: PermissionsChecker::Manual,
            agent_backend: AgentBackend::Codex,
            planner_backend: None,
            coding_backend: None,
            merge_strategy: MergeStrategy::Direct,
            allowed_authors: vec![],
            autostart: false,
            linear_team: None,
            linear_project: None,
            extra: BTreeMap::new(),
        }
    }
}
