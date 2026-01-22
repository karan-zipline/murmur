use std::path::Path;

use crate::config::ConfigFile;
use crate::paths::MurmurPaths;

pub fn detect_project_from_cwd(
    paths: &MurmurPaths,
    config: &ConfigFile,
    cwd: &Path,
) -> Option<String> {
    for project in &config.projects {
        let project_dir = paths.projects_dir.join(&project.name);
        if cwd.starts_with(&project_dir) {
            return Some(project.name.clone());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use tempfile::TempDir;

    use crate::config::{
        AgentBackend, ConfigFile, IssueBackend, MergeStrategy, PermissionsChecker, ProjectConfig,
    };
    use crate::paths::{compute_paths, MurmurPaths, PathInputs};

    fn mk_paths(tmp: &TempDir) -> MurmurPaths {
        compute_paths(PathInputs {
            home_dir: tmp.path().join("home"),
            xdg_config_home: Some(tmp.path().join("xdg")),
            xdg_runtime_dir: Some(tmp.path().join("run")),
            murmur_dir_override: Some(tmp.path().join("murmur")),
        })
    }

    fn mk_project(name: &str) -> ProjectConfig {
        ProjectConfig {
            name: name.to_owned(),
            remote_url: "file:///tmp/demo.git".to_owned(),
            max_agents: 1,
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

    #[test]
    fn detects_project_from_repo_path() {
        let tmp = TempDir::new().unwrap();
        let paths = mk_paths(&tmp);
        let config = ConfigFile {
            projects: vec![mk_project("demo")],
            ..ConfigFile::default()
        };

        let cwd = paths.projects_dir.join("demo").join("repo").join("src");
        assert_eq!(
            super::detect_project_from_cwd(&paths, &config, &cwd),
            Some("demo".to_owned())
        );
    }

    #[test]
    fn detects_project_from_worktree_path() {
        let tmp = TempDir::new().unwrap();
        let paths = mk_paths(&tmp);
        let config = ConfigFile {
            projects: vec![mk_project("demo")],
            ..ConfigFile::default()
        };

        let cwd = paths
            .projects_dir
            .join("demo")
            .join("worktrees")
            .join("wt-a-1")
            .join("nested");
        assert_eq!(
            super::detect_project_from_cwd(&paths, &config, &cwd),
            Some("demo".to_owned())
        );
    }

    #[test]
    fn returns_none_when_not_in_any_project() {
        let tmp = TempDir::new().unwrap();
        let paths = mk_paths(&tmp);
        let config = ConfigFile {
            projects: vec![mk_project("demo")],
            ..ConfigFile::default()
        };

        let cwd = PathBuf::from("/tmp/outside");
        assert_eq!(super::detect_project_from_cwd(&paths, &config, &cwd), None);
    }

    #[test]
    fn matches_project_dir_prefix() {
        let tmp = TempDir::new().unwrap();
        let paths = mk_paths(&tmp);
        let config = ConfigFile {
            projects: vec![mk_project("demo")],
            ..ConfigFile::default()
        };

        let cwd = paths.projects_dir.join("demo");
        assert_eq!(
            super::detect_project_from_cwd(&paths, &config, &cwd),
            Some("demo".to_owned())
        );
    }
}
