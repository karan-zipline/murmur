use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Context as _};

use crate::git::{agent_branch_name, parse_default_branch_from_remote_show, Git};

use super::{project_repo_dir, SharedState};

async fn maybe_test_merge_delay() {
    if !cfg!(debug_assertions) {
        return;
    }
    let Ok(ms) = std::env::var("FUGUE_TEST_MERGE_DELAY_MS") else {
        return;
    };
    let Ok(ms) = ms.parse::<u64>() else {
        return;
    };
    tokio::time::sleep(Duration::from_millis(ms)).await;
}

#[derive(Debug)]
pub(in crate::daemon) struct MergeSuccess {
    pub(in crate::daemon) sha: String,
    pub(in crate::daemon) branch: String,
}

#[derive(Debug)]
pub(in crate::daemon) enum MergeAttempt {
    Merged(MergeSuccess),
    Conflict { branch: String, error: String },
}

#[derive(Debug)]
pub(in crate::daemon) struct PullRequestPrep {
    pub(in crate::daemon) sha: String,
    pub(in crate::daemon) branch: String,
    pub(in crate::daemon) base_branch: String,
}

#[derive(Debug)]
pub(in crate::daemon) enum PullRequestAttempt {
    Ready(PullRequestPrep),
    Conflict { branch: String, error: String },
}

pub(in crate::daemon) async fn merge_lock_for_project(
    shared: &SharedState,
    project: &str,
) -> Arc<tokio::sync::Mutex<()>> {
    let mut locks = shared.merge_locks.lock().await;
    locks
        .entry(project.to_owned())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

async fn determine_default_branch(git: &Git, repo_dir: &Path) -> anyhow::Result<String> {
    let show = git.remote_show_origin(repo_dir).await?;
    let mut candidates = Vec::new();
    if let Some(b) = parse_default_branch_from_remote_show(&show) {
        let b = b.trim();
        if !b.is_empty() && b != "(unknown)" {
            candidates.push(b.to_owned());
        }
    }
    candidates.push("main".to_owned());
    candidates.push("master".to_owned());

    for candidate in candidates {
        let rev = format!("origin/{candidate}");
        if git.ref_exists(repo_dir, &rev).await? {
            return Ok(candidate);
        }
    }

    Err(anyhow!("could not determine default branch"))
}

pub(in crate::daemon) async fn merge_agent_branch_direct(
    shared: &SharedState,
    project: &str,
    agent_id: &str,
    worktree_dir: &Path,
) -> anyhow::Result<MergeAttempt> {
    let repo_dir = project_repo_dir(&shared.paths, project);
    if !repo_dir.join(".git").exists() {
        return Err(anyhow!("project repo not found: {}", repo_dir.display()));
    }

    maybe_test_merge_delay().await;

    shared.git.fetch_origin(&repo_dir).await?;

    let base_branch = determine_default_branch(&shared.git, &repo_dir).await?;
    let upstream = format!("origin/{base_branch}");

    if shared.git.checkout(&repo_dir, &base_branch).await.is_err() {
        shared
            .git
            .checkout_force(&repo_dir, &base_branch, &upstream)
            .await?;
    }
    shared.git.reset_hard(&repo_dir, &upstream).await?;

    if let Err(err) = shared.git.rebase_onto(worktree_dir, &upstream).await {
        shared.git.rebase_abort_best_effort(worktree_dir).await;
        return Ok(MergeAttempt::Conflict {
            branch: agent_branch_name(agent_id),
            error: format!("{err:#}"),
        });
    }

    let sha = shared.git.rev_parse(worktree_dir, "HEAD").await?;
    let branch = agent_branch_name(agent_id);

    shared.git.merge_ff_only(&repo_dir, &branch).await?;

    if let Err(err) = shared.git.push_ref(&repo_dir, "origin", &base_branch).await {
        let _ = shared.git.reset_hard(&repo_dir, &upstream).await;
        return Err(err).context("push base branch");
    }

    Ok(MergeAttempt::Merged(MergeSuccess { sha, branch }))
}

pub(in crate::daemon) async fn prepare_agent_branch_pull_request(
    shared: &SharedState,
    project: &str,
    agent_id: &str,
    worktree_dir: &Path,
) -> anyhow::Result<PullRequestAttempt> {
    let repo_dir = project_repo_dir(&shared.paths, project);
    if !repo_dir.join(".git").exists() {
        return Err(anyhow!("project repo not found: {}", repo_dir.display()));
    }

    maybe_test_merge_delay().await;

    shared.git.fetch_origin(&repo_dir).await?;

    let base_branch = determine_default_branch(&shared.git, &repo_dir).await?;
    let upstream = format!("origin/{base_branch}");

    if let Err(err) = shared.git.rebase_onto(worktree_dir, &upstream).await {
        shared.git.rebase_abort_best_effort(worktree_dir).await;
        return Ok(PullRequestAttempt::Conflict {
            branch: agent_branch_name(agent_id),
            error: format!("{err:#}"),
        });
    }

    let sha = shared.git.rev_parse(worktree_dir, "HEAD").await?;
    let branch = agent_branch_name(agent_id);
    let refspec = format!("{branch}:{branch}");

    shared
        .git
        .push_ref_force_with_lease(&repo_dir, "origin", &refspec)
        .await
        .context("push agent branch")?;

    Ok(PullRequestAttempt::Ready(PullRequestPrep {
        sha,
        branch,
        base_branch,
    }))
}
