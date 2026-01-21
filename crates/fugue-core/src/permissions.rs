use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Allow,
    Deny,
    Pass,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub tool: String,
    pub action: Action,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patterns: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ManagerConfig {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_patterns: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PermissionsFile {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<Rule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manager: Option<ManagerConfig>,
}

pub fn rewrite_pattern(pattern: &str, cwd: &str, home_dir: &str) -> String {
    if pattern.is_empty() {
        return pattern.to_owned();
    }

    if let Some(rest) = pattern.strip_prefix('~') {
        if rest.is_empty() {
            return home_dir.to_owned();
        }
        if let Some(rest) = rest.strip_prefix('/') {
            return join_paths(home_dir, rest);
        }
        return pattern.to_owned();
    }

    if let Some(rest) = pattern.strip_prefix("//") {
        return format!("/{rest}");
    }

    if pattern.starts_with('/') && !cwd.is_empty() {
        return format!("{cwd}{pattern}");
    }

    pattern.to_owned()
}

fn join_paths(base: &str, rest: &str) -> String {
    if rest.is_empty() {
        return base.to_owned();
    }
    if base.ends_with('/') {
        format!("{base}{rest}")
    } else {
        format!("{base}/{rest}")
    }
}

pub fn match_pattern(pattern: &str, value: &str) -> bool {
    if pattern.is_empty() || pattern == ":*" {
        return true;
    }

    if let Some(prefix) = pattern.strip_suffix(":*") {
        return value.starts_with(prefix);
    }

    pattern == value
}

pub fn resolve_primary_field(tool_name: &str, tool_input: &Value) -> String {
    let input = tool_input.as_object();
    let Some(input) = input else {
        return String::new();
    };

    match tool_name {
        "Bash" => input.get("command").and_then(Value::as_str).unwrap_or(""),
        "Read" | "Write" | "Edit" => input.get("file_path").and_then(Value::as_str).unwrap_or(""),
        "Glob" | "Grep" => input.get("pattern").and_then(Value::as_str).unwrap_or(""),
        "WebFetch" => input.get("url").and_then(Value::as_str).unwrap_or(""),
        "Task" => input.get("prompt").and_then(Value::as_str).unwrap_or(""),
        "Skill" => input.get("skill").and_then(Value::as_str).unwrap_or(""),
        "WebSearch" => input.get("query").and_then(Value::as_str).unwrap_or(""),
        "NotebookEdit" => input
            .get("notebook_path")
            .and_then(Value::as_str)
            .unwrap_or(""),
        _ => "",
    }
    .to_owned()
}

pub fn evaluate_rules(
    rules: &[Rule],
    tool_name: &str,
    tool_input: &Value,
    cwd: &str,
    home_dir: &str,
) -> (Action, bool) {
    let primary_field = resolve_primary_field(tool_name, tool_input);

    for rule in rules {
        if rule.tool != tool_name {
            continue;
        }

        if rule.script.as_deref().is_some_and(|s| !s.trim().is_empty()) {
            continue;
        }

        let mut matched = false;

        if let Some(pattern) = rule.pattern.as_deref().filter(|s| !s.is_empty()) {
            let rewritten = rewrite_pattern(pattern, cwd, home_dir);
            matched = match_pattern(&rewritten, &primary_field);
        } else if !rule.patterns.is_empty() {
            for p in &rule.patterns {
                let rewritten = rewrite_pattern(p, cwd, home_dir);
                if match_pattern(&rewritten, &primary_field) {
                    matched = true;
                    break;
                }
            }
        } else {
            matched = true;
        }

        if matched {
            if rule.action == Action::Pass {
                continue;
            }
            return (rule.action, true);
        }
    }

    (Action::Pass, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrite_pattern_examples() {
        let home = "/home/alice";
        let cwd = "/home/alice/project";

        let cases = [
            ("/:*", cwd, "/home/alice/project/:*"),
            ("/src/:*", cwd, "/home/alice/project/src/:*"),
            ("//:*", cwd, "/:*"),
            ("//etc/passwd", cwd, "/etc/passwd"),
            ("~", cwd, "/home/alice"),
            ("~/:*", cwd, "/home/alice/:*"),
            ("~/.config/:*", cwd, "/home/alice/.config/:*"),
            ("~otheruser/file", cwd, "~otheruser/file"),
            ("git :*", cwd, "git :*"),
            (":*", cwd, ":*"),
            ("", cwd, ""),
            ("/:*", "", "/:*"),
            ("/", cwd, "/home/alice/project/"),
            ("//", cwd, "/"),
        ];

        for (pattern, cwd, want) in cases {
            let got = rewrite_pattern(pattern, cwd, home);
            assert_eq!(got, want, "pattern={pattern:?} cwd={cwd:?}");
        }
    }

    #[test]
    fn match_pattern_examples() {
        let cases = [
            ("", "anything", true),
            (":*", "anything", true),
            ("git :*", "git status", true),
            ("git :*", "gitignore", false),
            ("git:*", "git", true),
            ("git:*", "git status", true),
            ("git status", "git status", true),
            ("git status", "git status --short", false),
        ];

        for (pattern, value, want) in cases {
            let got = match_pattern(pattern, value);
            assert_eq!(got, want, "pattern={pattern:?} value={value:?}");
        }
    }

    #[test]
    fn resolve_primary_field_examples() {
        let cases = [
            (
                "Bash",
                serde_json::json!({"command":"git status"}),
                "git status",
            ),
            (
                "Read",
                serde_json::json!({"file_path":"/etc/passwd"}),
                "/etc/passwd",
            ),
            (
                "Write",
                serde_json::json!({"file_path":"/tmp/test.txt","content":"hello"}),
                "/tmp/test.txt",
            ),
            (
                "Edit",
                serde_json::json!({"file_path":"/src/main.go"}),
                "/src/main.go",
            ),
            ("Glob", serde_json::json!({"pattern":"**/*.rs"}), "**/*.rs"),
            ("Grep", serde_json::json!({"pattern":"TODO"}), "TODO"),
            (
                "WebFetch",
                serde_json::json!({"url":"https://example.com"}),
                "https://example.com",
            ),
            ("Task", serde_json::json!({"prompt":"search"}), "search"),
            (
                "Skill",
                serde_json::json!({"skill":"ticket:ready"}),
                "ticket:ready",
            ),
            (
                "WebSearch",
                serde_json::json!({"query":"rust tutorials"}),
                "rust tutorials",
            ),
            (
                "NotebookEdit",
                serde_json::json!({"notebook_path":"/home/alice/analysis.ipynb"}),
                "/home/alice/analysis.ipynb",
            ),
            ("Unknown", serde_json::json!({"foo":"bar"}), ""),
            ("Bash", serde_json::json!({}), ""),
        ];

        for (tool, input, want) in cases {
            let got = resolve_primary_field(tool, &input);
            assert_eq!(got, want, "tool={tool:?} input={input:?}");
        }
    }

    #[test]
    fn evaluate_rules_obeys_precedence_and_patterns() {
        let home = "/home/alice";
        let cwd = "/home/alice/project";

        let rules = vec![
            Rule {
                tool: "Bash".to_owned(),
                action: Action::Allow,
                pattern: None,
                patterns: vec!["git :*".to_owned(), "cargo :*".to_owned()],
                script: None,
            },
            Rule {
                tool: "Bash".to_owned(),
                action: Action::Deny,
                pattern: Some("rm :*".to_owned()),
                patterns: Vec::new(),
                script: None,
            },
            Rule {
                tool: "Read".to_owned(),
                action: Action::Allow,
                pattern: None,
                patterns: Vec::new(),
                script: None,
            },
        ];

        let (action, matched) = evaluate_rules(
            &rules,
            "Bash",
            &serde_json::json!({"command":"git status"}),
            cwd,
            home,
        );
        assert_eq!(action, Action::Allow);
        assert!(matched);

        let (action, matched) = evaluate_rules(
            &rules,
            "Bash",
            &serde_json::json!({"command":"rm -rf /"}),
            cwd,
            home,
        );
        assert_eq!(action, Action::Deny);
        assert!(matched);

        let (action, matched) = evaluate_rules(
            &rules,
            "Bash",
            &serde_json::json!({"command":"python script.py"}),
            cwd,
            home,
        );
        assert_eq!(action, Action::Pass);
        assert!(!matched);

        let (action, matched) = evaluate_rules(
            &rules,
            "Read",
            &serde_json::json!({"file_path":"/any/file.txt"}),
            cwd,
            home,
        );
        assert_eq!(action, Action::Allow);
        assert!(matched);
    }

    #[test]
    fn evaluate_rules_rewrites_cwd_scoped_paths() {
        let home = "/home/alice";
        let cwd = "/home/alice/project";

        let rules = vec![Rule {
            tool: "Write".to_owned(),
            action: Action::Allow,
            pattern: Some("/src/:*".to_owned()),
            patterns: Vec::new(),
            script: None,
        }];

        let (action, matched) = evaluate_rules(
            &rules,
            "Write",
            &serde_json::json!({"file_path":"/home/alice/project/src/main.rs"}),
            cwd,
            home,
        );
        assert_eq!(action, Action::Allow);
        assert!(matched);

        let (action, matched) = evaluate_rules(
            &rules,
            "Write",
            &serde_json::json!({"file_path":"/etc/passwd"}),
            cwd,
            home,
        );
        assert_eq!(action, Action::Pass);
        assert!(!matched);
    }
}
