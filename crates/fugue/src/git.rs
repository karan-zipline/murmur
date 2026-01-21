use std::path::Path;

use anyhow::{anyhow, Context as _};

#[derive(Debug, Clone)]
pub struct Git {
    exe: String,
}

impl Default for Git {
    fn default() -> Self {
        Self::new("git")
    }
}

impl Git {
    pub fn new<S: Into<String>>(exe: S) -> Self {
        Self { exe: exe.into() }
    }

    pub async fn clone_repo(&self, remote_url: &str, repo_dir: &Path) -> anyhow::Result<()> {
        let output = tokio::process::Command::new(&self.exe)
            .arg("clone")
            .arg("--")
            .arg(remote_url)
            .arg(repo_dir)
            .output()
            .await
            .context("spawn git clone")?;

        ensure_success(output, "git clone")
    }

    pub async fn remote_origin_url(&self, repo_dir: &Path) -> anyhow::Result<String> {
        let output = tokio::process::Command::new(&self.exe)
            .arg("-C")
            .arg(repo_dir)
            .args(["remote", "get-url", "origin"])
            .output()
            .await
            .context("spawn git remote get-url")?;

        ensure_success_with_stdout(output, "git remote get-url")
    }

    pub async fn fetch_origin(&self, repo_dir: &Path) -> anyhow::Result<()> {
        let output = tokio::process::Command::new(&self.exe)
            .arg("-C")
            .arg(repo_dir)
            .args(["fetch", "--prune", "origin"])
            .output()
            .await
            .context("spawn git fetch")?;

        ensure_success(output, "git fetch")
    }

    pub async fn remote_show_origin(&self, repo_dir: &Path) -> anyhow::Result<String> {
        let output = tokio::process::Command::new(&self.exe)
            .arg("-C")
            .arg(repo_dir)
            .args(["remote", "show", "origin"])
            .output()
            .await
            .context("spawn git remote show origin")?;

        ensure_success_with_stdout(output, "git remote show origin")
    }

    pub async fn worktree_add(
        &self,
        repo_dir: &Path,
        worktree_dir: &Path,
        branch: &str,
        start_point: &str,
    ) -> anyhow::Result<()> {
        if self.ref_exists(repo_dir, branch).await? {
            let output = tokio::process::Command::new(&self.exe)
                .arg("-C")
                .arg(repo_dir)
                .args(["worktree", "add"])
                .arg("--")
                .arg(worktree_dir)
                .arg(branch)
                .output()
                .await
                .context("spawn git worktree add (existing branch)")?;

            return ensure_success(output, "git worktree add");
        }

        let output = tokio::process::Command::new(&self.exe)
            .arg("-C")
            .arg(repo_dir)
            .args(["worktree", "add", "-b", branch])
            .arg("--")
            .arg(worktree_dir)
            .arg(start_point)
            .output()
            .await
            .context("spawn git worktree add (new branch)")?;

        ensure_success(output, "git worktree add")
    }

    pub async fn worktree_remove(
        &self,
        repo_dir: &Path,
        worktree_dir: &Path,
    ) -> anyhow::Result<()> {
        let output = tokio::process::Command::new(&self.exe)
            .arg("-C")
            .arg(repo_dir)
            .args(["worktree", "remove", "--force"])
            .arg("--")
            .arg(worktree_dir)
            .output()
            .await
            .context("spawn git worktree remove")?;

        ensure_success(output, "git worktree remove")
    }

    pub async fn worktree_list(&self, repo_dir: &Path) -> anyhow::Result<String> {
        let output = tokio::process::Command::new(&self.exe)
            .arg("-C")
            .arg(repo_dir)
            .args(["worktree", "list"])
            .output()
            .await
            .context("spawn git worktree list")?;

        ensure_success_with_stdout(output, "git worktree list")
    }

    pub async fn list_refs_short(
        &self,
        repo_dir: &Path,
        refs: &str,
    ) -> anyhow::Result<Vec<String>> {
        let output = tokio::process::Command::new(&self.exe)
            .arg("-C")
            .arg(repo_dir)
            .args(["for-each-ref", "--format=%(refname:short)"])
            .arg(refs)
            .output()
            .await
            .context("spawn git for-each-ref")?;

        let stdout = ensure_success_with_stdout(output, "git for-each-ref")?;
        Ok(stdout
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .map(|l| l.to_owned())
            .collect())
    }

    pub async fn ref_exists(&self, repo_dir: &Path, rev: &str) -> anyhow::Result<bool> {
        let output = tokio::process::Command::new(&self.exe)
            .arg("-C")
            .arg(repo_dir)
            .args(["rev-parse", "--verify", "--quiet"])
            .arg(rev)
            .output()
            .await
            .context("spawn git rev-parse")?;

        Ok(output.status.success())
    }

    pub async fn add_path(&self, repo_dir: &Path, path: &Path) -> anyhow::Result<()> {
        let output = tokio::process::Command::new(&self.exe)
            .arg("-C")
            .arg(repo_dir)
            .args(["add", "--"])
            .arg(path)
            .output()
            .await
            .context("spawn git add")?;

        ensure_success(output, "git add")
    }

    pub async fn diff_cached_has_changes(&self, repo_dir: &Path) -> anyhow::Result<bool> {
        let output = tokio::process::Command::new(&self.exe)
            .arg("-C")
            .arg(repo_dir)
            .args(["diff", "--cached", "--quiet"])
            .output()
            .await
            .context("spawn git diff --cached")?;

        if output.status.success() {
            return Ok(false);
        }
        if output.status.code() == Some(1) {
            return Ok(true);
        }

        Err(anyhow!(
            "git diff --cached exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ))
    }

    pub async fn commit(&self, repo_dir: &Path, message: &str) -> anyhow::Result<()> {
        let output = tokio::process::Command::new(&self.exe)
            .arg("-C")
            .arg(repo_dir)
            .args(["commit", "-m", message])
            .output()
            .await
            .context("spawn git commit")?;

        ensure_success(output, "git commit")
    }

    pub async fn push_head(&self, repo_dir: &Path) -> anyhow::Result<()> {
        let output = tokio::process::Command::new(&self.exe)
            .arg("-C")
            .arg(repo_dir)
            .args(["push", "origin", "HEAD"])
            .output()
            .await
            .context("spawn git push")?;

        ensure_success(output, "git push")
    }

    pub async fn reset_soft_head1(&self, repo_dir: &Path) -> anyhow::Result<()> {
        let output = tokio::process::Command::new(&self.exe)
            .arg("-C")
            .arg(repo_dir)
            .args(["reset", "--soft", "HEAD~1"])
            .output()
            .await
            .context("spawn git reset --soft")?;

        ensure_success(output, "git reset --soft")
    }

    pub async fn checkout(&self, repo_dir: &Path, branch: &str) -> anyhow::Result<()> {
        let output = tokio::process::Command::new(&self.exe)
            .arg("-C")
            .arg(repo_dir)
            .args(["checkout", branch])
            .output()
            .await
            .context("spawn git checkout")?;

        ensure_success(output, "git checkout")
    }

    pub async fn checkout_force(
        &self,
        repo_dir: &Path,
        branch: &str,
        start_point: &str,
    ) -> anyhow::Result<()> {
        let output = tokio::process::Command::new(&self.exe)
            .arg("-C")
            .arg(repo_dir)
            .args(["checkout", "-B", branch, start_point])
            .output()
            .await
            .context("spawn git checkout -B")?;

        ensure_success(output, "git checkout -B")
    }

    pub async fn reset_hard(&self, repo_dir: &Path, rev: &str) -> anyhow::Result<()> {
        let output = tokio::process::Command::new(&self.exe)
            .arg("-C")
            .arg(repo_dir)
            .args(["reset", "--hard", rev])
            .output()
            .await
            .context("spawn git reset --hard")?;

        ensure_success(output, "git reset --hard")
    }

    pub async fn rev_parse(&self, repo_dir: &Path, rev: &str) -> anyhow::Result<String> {
        let output = tokio::process::Command::new(&self.exe)
            .arg("-C")
            .arg(repo_dir)
            .args(["rev-parse", rev])
            .output()
            .await
            .context("spawn git rev-parse")?;

        ensure_success_with_stdout(output, "git rev-parse")
    }

    pub async fn merge_ff_only(&self, repo_dir: &Path, rev: &str) -> anyhow::Result<()> {
        let output = tokio::process::Command::new(&self.exe)
            .arg("-C")
            .arg(repo_dir)
            .args(["merge", "--ff-only", rev])
            .output()
            .await
            .context("spawn git merge --ff-only")?;

        ensure_success(output, "git merge --ff-only")
    }

    pub async fn push_ref(
        &self,
        repo_dir: &Path,
        remote: &str,
        refspec: &str,
    ) -> anyhow::Result<()> {
        let output = tokio::process::Command::new(&self.exe)
            .arg("-C")
            .arg(repo_dir)
            .args(["push", remote, refspec])
            .output()
            .await
            .context("spawn git push")?;

        ensure_success(output, "git push")
    }

    pub async fn push_ref_force_with_lease(
        &self,
        repo_dir: &Path,
        remote: &str,
        refspec: &str,
    ) -> anyhow::Result<()> {
        let output = tokio::process::Command::new(&self.exe)
            .arg("-C")
            .arg(repo_dir)
            .args(["push", "--force-with-lease", remote, refspec])
            .output()
            .await
            .context("spawn git push --force-with-lease")?;

        ensure_success(output, "git push --force-with-lease")
    }

    pub async fn rebase_onto(&self, worktree_dir: &Path, upstream: &str) -> anyhow::Result<()> {
        let output = tokio::process::Command::new(&self.exe)
            .arg("-C")
            .arg(worktree_dir)
            .args(["rebase", upstream])
            .output()
            .await
            .context("spawn git rebase")?;

        ensure_success_with_output(output, "git rebase")
    }

    pub async fn rebase_abort_best_effort(&self, worktree_dir: &Path) {
        let _ = tokio::process::Command::new(&self.exe)
            .arg("-C")
            .arg(worktree_dir)
            .args(["rebase", "--abort"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .await;
    }
}

pub fn parse_default_branch_from_remote_show(output: &str) -> Option<String> {
    for line in output.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("HEAD branch:") else {
            continue;
        };
        let branch = rest.trim();
        if branch.is_empty() {
            continue;
        }
        return Some(branch.to_owned());
    }
    None
}

pub fn agent_branch_name(agent_id: &str) -> String {
    format!("fugue/{agent_id}")
}

fn ensure_success(output: std::process::Output, context: &str) -> anyhow::Result<()> {
    if output.status.success() {
        Ok(())
    } else {
        Err(anyhow!(
            "{context} exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

fn ensure_success_with_stdout(
    output: std::process::Output,
    context: &str,
) -> anyhow::Result<String> {
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(anyhow!(
            "{context} exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

fn ensure_success_with_output(output: std::process::Output, context: &str) -> anyhow::Result<()> {
    if output.status.success() {
        return Ok(());
    }

    let mut msg = String::new();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stdout.trim().is_empty() {
        msg.push_str(stdout.trim());
    }
    if !stderr.trim().is_empty() {
        if !msg.is_empty() {
            msg.push('\n');
        }
        msg.push_str(stderr.trim());
    }

    Err(anyhow!("{context} exited {}: {msg}", output.status))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_branch_from_remote_show() {
        let fixture = r#"
* remote origin
  Fetch URL: git@github.com:example/repo.git
  Push  URL: git@github.com:example/repo.git
  HEAD branch: main
  Remote branch:
    main tracked
"#;
        assert_eq!(
            parse_default_branch_from_remote_show(fixture).as_deref(),
            Some("main")
        );
    }

    #[test]
    fn default_branch_none_when_missing() {
        assert_eq!(parse_default_branch_from_remote_show("x"), None);
    }
}
