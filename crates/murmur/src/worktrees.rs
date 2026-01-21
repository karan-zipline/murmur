use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context as _};
use murmur_core::paths::{safe_join, MurmurPaths};

use crate::git::{agent_branch_name, parse_default_branch_from_remote_show, Git};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub dir: PathBuf,
    pub branch: String,
    pub base_branch: String,
}

#[derive(Debug, Clone)]
pub struct WorktreeManager<'a> {
    git: &'a Git,
    paths: &'a MurmurPaths,
}

impl<'a> WorktreeManager<'a> {
    pub fn new(git: &'a Git, paths: &'a MurmurPaths) -> Self {
        Self { git, paths }
    }

    pub fn project_dir(&self, project: &str) -> PathBuf {
        self.paths.projects_dir.join(project)
    }

    pub fn project_repo_dir(&self, project: &str) -> PathBuf {
        self.project_dir(project).join("repo")
    }

    pub fn project_worktrees_dir(&self, project: &str) -> PathBuf {
        self.project_dir(project).join("worktrees")
    }

    pub async fn create_agent_worktree(
        &self,
        project: &str,
        agent_id: &str,
    ) -> anyhow::Result<Worktree> {
        let repo_dir = self.project_repo_dir(project);
        if !repo_dir.join(".git").exists() {
            return Err(anyhow!("project repo not found: {}", repo_dir.display()));
        }

        self.git.fetch_origin(&repo_dir).await.ok();

        let show = self.git.remote_show_origin(&repo_dir).await?;
        let mut candidates = Vec::new();
        if let Some(b) = parse_default_branch_from_remote_show(&show) {
            let b = b.trim();
            if !b.is_empty() && b != "(unknown)" {
                candidates.push(b.to_owned());
            }
        }
        candidates.push("main".to_owned());
        candidates.push("master".to_owned());

        let mut base_branch = None;
        for candidate in candidates {
            let rev = format!("origin/{candidate}");
            if self.git.ref_exists(&repo_dir, &rev).await? {
                base_branch = Some(candidate);
                break;
            }
        }
        let base_branch =
            base_branch.ok_or_else(|| anyhow!("could not determine default branch"))?;

        let branch = agent_branch_name(agent_id);
        let start_point = format!("origin/{base_branch}");

        let worktrees_dir = self.project_worktrees_dir(project);
        tokio::fs::create_dir_all(&worktrees_dir)
            .await
            .with_context(|| format!("create worktrees dir: {}", worktrees_dir.display()))?;

        let wt_name = format!("wt-{agent_id}");
        let dir = safe_join(&worktrees_dir, &wt_name)
            .map_err(|e| anyhow!("invalid worktree dir name {wt_name}: {e}"))?;

        if dir.exists() {
            return Err(anyhow!("worktree already exists: {}", dir.display()));
        }

        self.git
            .worktree_add(&repo_dir, &dir, &branch, &start_point)
            .await?;

        Ok(Worktree {
            dir,
            branch,
            base_branch,
        })
    }

    pub async fn remove_worktree(&self, project: &str, worktree_dir: &Path) -> anyhow::Result<()> {
        let repo_dir = self.project_repo_dir(project);
        if !repo_dir.join(".git").exists() {
            return Err(anyhow!("project repo not found: {}", repo_dir.display()));
        }

        self.git.worktree_remove(&repo_dir, worktree_dir).await?;
        Ok(())
    }
}
