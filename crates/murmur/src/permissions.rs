use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use murmur_core::paths::{safe_join, MurmurPaths};
use murmur_core::permissions::{Action, PermissionsFile, Rule};

pub async fn load_rules(paths: &MurmurPaths, project: Option<&str>) -> Result<Vec<Rule>> {
    let mut rules = Vec::new();

    if let Some(project) = project.map(str::trim).filter(|s| !s.is_empty()) {
        let path = project_permissions_path(paths, project)?;
        if let Some(cfg) = load_permissions_file(&path).await? {
            rules.extend(cfg.rules);
        }
    }

    if let Some(cfg) = load_permissions_file(&paths.permissions_file).await? {
        rules.extend(cfg.rules);
    }

    rules.extend(default_rules());

    Ok(rules)
}

pub async fn load_manager_allowed_patterns(paths: &MurmurPaths) -> Result<Vec<String>> {
    const DEFAULT: &[&str] = &["mm:*"];

    let Some(cfg) = load_permissions_file(&paths.permissions_file).await? else {
        return Ok(DEFAULT.iter().map(|s| s.to_string()).collect());
    };

    let Some(manager) = cfg.manager else {
        return Ok(DEFAULT.iter().map(|s| s.to_string()).collect());
    };

    let patterns = manager
        .allowed_patterns
        .into_iter()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();

    if patterns.is_empty() {
        Ok(DEFAULT.iter().map(|s| s.to_string()).collect())
    } else {
        Ok(patterns)
    }
}

fn default_rules() -> Vec<Rule> {
    fn rule(tool: &str, action: Action, patterns: &[&str]) -> Rule {
        Rule {
            tool: tool.to_owned(),
            action,
            pattern: None,
            patterns: patterns.iter().map(|s| s.to_string()).collect(),
            script: None,
        }
    }

    vec![
        rule("TodoWrite", Action::Allow, &[]),
        rule("EnterPlanMode", Action::Allow, &[]),
        rule("ExitPlanMode", Action::Allow, &[]),
        rule("AskUserQuestion", Action::Allow, &[]),
        rule(
            "Bash",
            Action::Deny,
            &[
                "rm :*",
                "sudo:*",
                "chmod :*",
                "chown :*",
                "dd :*",
                "mkfs :*",
                "mount :*",
                "umount :*",
                "shutdown:*",
                "reboot:*",
                "pkill :*",
                "kill :*",
                "git push:*",
                "git reset:*",
                "git clean:*",
            ],
        ),
        rule(
            "Bash",
            Action::Allow,
            &[
                "tk:*",
                "mm :*",
                "git status:*",
                "git diff:*",
                "git log:*",
                "git show:*",
                "git branch:*",
                "git rev-parse:*",
                "git describe:*",
                "git blame:*",
                "git remote:*",
                "ls:*",
                "tree:*",
                "cat:*",
                "diff:*",
                "grep:*",
                "head:*",
                "tail:*",
                "wc:*",
            ],
        ),
        rule("Glob", Action::Allow, &[]),
        rule("Grep", Action::Allow, &[]),
        rule("Read", Action::Allow, &[]),
        rule("Write", Action::Allow, &["/:*"]),
        rule("Edit", Action::Allow, &["/:*"]),
        rule("WebSearch", Action::Allow, &[]),
        rule(
            "WebFetch",
            Action::Allow,
            &[
                "https://docs.rs:*",
                "https://pkg.go.dev:*",
                "https://github.com:*",
                "https://developer.mozilla.org:*",
            ],
        ),
    ]
}

fn project_permissions_path(paths: &MurmurPaths, project: &str) -> Result<PathBuf> {
    let project_dir = safe_join(&paths.projects_dir, project)
        .map_err(|e| anyhow::anyhow!("invalid project name {project:?}: {e}"))?;
    Ok(project_dir.join("permissions.toml"))
}

async fn load_permissions_file(path: &Path) -> Result<Option<PermissionsFile>> {
    match tokio::fs::read_to_string(path).await {
        Ok(s) => {
            let cfg: PermissionsFile =
                toml::from_str(&s).with_context(|| format!("parse {}", path.display()))?;
            Ok(Some(cfg))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(err).with_context(|| format!("read {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use murmur_core::paths::{compute_paths, PathInputs};
    use tempfile::TempDir;

    fn test_paths(dir: &TempDir) -> MurmurPaths {
        compute_paths(PathInputs {
            home_dir: dir.path().to_path_buf(),
            xdg_config_home: Some(dir.path().join("xdg")),
            xdg_runtime_dir: Some(dir.path().join("run")),
            murmur_dir_override: Some(dir.path().join("murmur")),
        })
    }

    #[tokio::test]
    async fn loads_project_rules_before_global() {
        let dir = TempDir::new().unwrap();
        let paths = test_paths(&dir);

        tokio::fs::create_dir_all(paths.permissions_file.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(
            &paths.permissions_file,
            r#"
[[rules]]
tool = "Bash"
action = "allow"
pattern = "ls :*"
"#,
        )
        .await
        .unwrap();

        let project_dir = paths.projects_dir.join("demo");
        tokio::fs::create_dir_all(&project_dir).await.unwrap();
        tokio::fs::write(
            project_dir.join("permissions.toml"),
            r#"
[[rules]]
tool = "Bash"
action = "deny"
pattern = "rm :*"
"#,
        )
        .await
        .unwrap();

        let rules = load_rules(&paths, Some("demo")).await.unwrap();
        assert!(rules.len() >= 2);
        assert_eq!(rules[0].tool, "Bash");
        assert_eq!(rules[0].action, murmur_core::permissions::Action::Deny);
        assert_eq!(rules[1].action, murmur_core::permissions::Action::Allow);
    }

    #[tokio::test]
    async fn missing_files_are_ok() {
        let dir = TempDir::new().unwrap();
        let paths = test_paths(&dir);

        let rules = load_rules(&paths, Some("demo")).await.unwrap();
        assert!(
            rules
                .iter()
                .any(|r| r.tool == "Read" && r.action == murmur_core::permissions::Action::Allow),
            "should include default rules"
        );
    }

    #[tokio::test]
    async fn manager_allowed_patterns_defaults_when_missing() {
        let dir = TempDir::new().unwrap();
        let paths = test_paths(&dir);

        let patterns = load_manager_allowed_patterns(&paths).await.unwrap();
        assert!(patterns.iter().any(|p| p == "mm:*"));
    }

    #[tokio::test]
    async fn manager_allowed_patterns_loads_from_global_permissions() {
        let dir = TempDir::new().unwrap();
        let paths = test_paths(&dir);

        tokio::fs::create_dir_all(paths.permissions_file.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(
            &paths.permissions_file,
            r#"
[manager]
allowed_patterns = ["mm:*", "git :*"]
"#,
        )
        .await
        .unwrap();

        let patterns = load_manager_allowed_patterns(&paths).await.unwrap();
        assert_eq!(patterns, vec!["mm:*".to_owned(), "git :*".to_owned()]);
    }
}
