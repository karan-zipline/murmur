use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context as _};
use fs2::FileExt as _;
use murmur_core::issue::{
    compute_ready_issues, tk_format_issue, tk_parse_issue, tk_upsert_comment, Comment, CreateParams, Issue,
    ListFilter, Status, UpdateParams,
};
use murmur_core::paths::safe_join;

use crate::git::Git;

#[derive(Debug, Clone)]
pub struct TkBackend<'a> {
    git: &'a Git,
    repo_dir: PathBuf,
    tickets_dir: PathBuf,
    id_prefix: String,
}

impl<'a> TkBackend<'a> {
    pub async fn new(git: &'a Git, repo_dir: PathBuf) -> anyhow::Result<Self> {
        let tickets_dir = repo_dir.join(".murmur").join("tickets");
        let id_prefix = detect_prefix(&tickets_dir)
            .await
            .unwrap_or_else(|| "issue-".to_owned());
        Ok(Self {
            git,
            repo_dir,
            tickets_dir,
            id_prefix,
        })
    }

    pub fn tickets_dir(&self) -> &Path {
        &self.tickets_dir
    }

    pub async fn get(&self, id: &str) -> anyhow::Result<Issue> {
        let path = self.issue_path(id)?;
        let data = tokio::fs::read_to_string(&path)
            .await
            .with_context(|| format!("read issue: {}", path.display()))?;

        tk_parse_issue(&data).map_err(|e| anyhow!("parse issue: {e}"))
    }

    pub async fn list(&self, filter: ListFilter) -> anyhow::Result<Vec<Issue>> {
        let mut issues = Vec::new();

        let mut dir = match tokio::fs::read_dir(&self.tickets_dir).await {
            Ok(v) => v,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(issues),
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("read tickets dir: {}", self.tickets_dir.display()));
            }
        };

        while let Some(entry) = dir.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                continue;
            }

            let data = match tokio::fs::read_to_string(&path).await {
                Ok(v) => v,
                Err(_) => continue,
            };

            let issue = match tk_parse_issue(&data) {
                Ok(v) => v,
                Err(_) => continue,
            };

            if filter.matches(&issue) {
                issues.push(issue);
            }
        }

        Ok(issues)
    }

    pub async fn ready(&self) -> anyhow::Result<Vec<Issue>> {
        let open = self
            .list(ListFilter {
                status: vec![Status::Open],
                labels: vec![],
            })
            .await?;
        Ok(compute_ready_issues(open))
    }

    pub async fn create(&self, now_ms: u64, params: CreateParams) -> anyhow::Result<Issue> {
        tokio::fs::create_dir_all(&self.tickets_dir)
            .await
            .with_context(|| format!("create tickets dir: {}", self.tickets_dir.display()))?;

        let id = self.generate_id().await?;
        let issue_type = if params.issue_type.trim().is_empty() {
            "task".to_owned()
        } else {
            params.issue_type
        };

        let issue = Issue {
            id: id.clone(),
            title: params.title,
            description: params.description,
            status: Status::Open,
            priority: params.priority,
            issue_type,
            dependencies: params.dependencies,
            labels: params.labels,
            links: params.links,
            created_at_ms: now_ms,
        };

        self.write(&issue).await?;
        Ok(issue)
    }

    pub async fn update(
        &self,
        now_ms: u64,
        id: &str,
        params: UpdateParams,
    ) -> anyhow::Result<Issue> {
        let mut issue = self.get(id).await?;

        if let Some(v) = params.title {
            issue.title = v;
        }
        if let Some(v) = params.description {
            issue.description = v;
        }
        if let Some(v) = params.status {
            issue.status = v;
        }
        if let Some(v) = params.priority {
            issue.priority = v;
        }
        if let Some(v) = params.issue_type {
            issue.issue_type = v;
        }
        if let Some(v) = params.labels {
            issue.labels = v;
        }
        if let Some(v) = params.dependencies {
            issue.dependencies = v;
        }
        if let Some(v) = params.links {
            issue.links = v;
        }

        if issue.created_at_ms == 0 {
            issue.created_at_ms = now_ms;
        }

        self.write(&issue).await?;
        Ok(issue)
    }

    pub async fn close(&self, now_ms: u64, id: &str) -> anyhow::Result<()> {
        let _ = self
            .update(
                now_ms,
                id,
                UpdateParams {
                    status: Some(Status::Closed),
                    ..Default::default()
                },
            )
            .await?;
        Ok(())
    }

    pub async fn comment(&self, now_ms: u64, id: &str, body: &str) -> anyhow::Result<()> {
        let mut issue = self.get(id).await?;

        let timestamp = format_comment_timestamp(now_ms);
        let comment = format!("**{timestamp}**: {}", body.trim());
        issue.description = tk_upsert_comment(&issue.description, &comment);

        self.write(&issue).await?;
        Ok(())
    }

    pub async fn commit(&self, message: &str) -> anyhow::Result<()> {
        tokio::fs::create_dir_all(&self.tickets_dir)
            .await
            .with_context(|| format!("create tickets dir: {}", self.tickets_dir.display()))?;

        let lock_path = self.tickets_dir.join(".lock");
        let lock_file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .with_context(|| format!("open lock file: {}", lock_path.display()))?;
        lock_file
            .lock_exclusive()
            .with_context(|| format!("lock {}", lock_path.display()))?;

        let result = self.commit_unlocked(message).await;

        let _ = lock_file.unlock();
        result
    }

    /// List comments on an issue. Returns an error as tk backend stores comments in the issue body.
    pub async fn list_comments(
        &self,
        _issue_id: &str,
        _since_ms: Option<u64>,
    ) -> anyhow::Result<Vec<Comment>> {
        Err(anyhow!("list_comments not supported by tk backend"))
    }

    async fn commit_unlocked(&self, message: &str) -> anyhow::Result<()> {
        self.git
            .add_path(&self.repo_dir, &self.tickets_dir)
            .await
            .context("git add tickets")?;

        let has_changes = self
            .git
            .diff_cached_has_changes(&self.repo_dir)
            .await
            .context("git diff --cached")?;
        if !has_changes {
            return Ok(());
        }

        self.git
            .commit(&self.repo_dir, message)
            .await
            .context("git commit")?;

        if let Err(err) = self.git.push_head(&self.repo_dir).await.context("git push") {
            let _ = self.git.reset_soft_head1(&self.repo_dir).await;
            return Err(err);
        }

        Ok(())
    }

    async fn write(&self, issue: &Issue) -> anyhow::Result<()> {
        tokio::fs::create_dir_all(&self.tickets_dir)
            .await
            .with_context(|| format!("create tickets dir: {}", self.tickets_dir.display()))?;

        let content = tk_format_issue(issue).map_err(|e| anyhow!("format issue: {e}"))?;
        let path = self.issue_path(&issue.id)?;
        tokio::fs::write(&path, content)
            .await
            .with_context(|| format!("write issue: {}", path.display()))?;
        Ok(())
    }

    fn issue_path(&self, id: &str) -> anyhow::Result<PathBuf> {
        let file = format!("{}.md", id.trim());
        let path = safe_join(&self.tickets_dir, &file)
            .map_err(|e| anyhow!("invalid issue id {id}: {e}"))?;
        Ok(path)
    }

    async fn generate_id(&self) -> anyhow::Result<String> {
        for _ in 0..16 {
            let suffix = random_hex_3()?;
            let id = format!("{}{}", self.id_prefix, suffix);
            if !self.issue_path(&id)?.exists() {
                return Ok(id);
            }
        }
        Err(anyhow!("failed to generate unique issue id"))
    }
}

async fn detect_prefix(tickets_dir: &Path) -> Option<String> {
    let mut dir = match tokio::fs::read_dir(tickets_dir).await {
        Ok(v) => v,
        Err(_) => return None,
    };

    while let Ok(Some(entry)) = dir.next_entry().await {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let name = path.file_stem()?.to_string_lossy();
        if let Some(idx) = name.rfind('-') {
            return Some(name[..idx + 1].to_owned());
        }
    }

    None
}

fn random_hex_3() -> anyhow::Result<String> {
    let mut bytes = [0u8; 2];
    getrandom::getrandom(&mut bytes).map_err(|e| anyhow!("getrandom failed: {e:?}"))?;
    let val = u16::from_le_bytes(bytes) & 0x0fff;
    Ok(format!("{val:03x}"))
}

fn format_comment_timestamp(now_ms: u64) -> String {
    let secs = (now_ms / 1000) as i64;
    let nanos = ((now_ms % 1000) * 1_000_000) as u32;

    let dt = time::OffsetDateTime::from_unix_timestamp(secs)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
        .replace_nanosecond(nanos)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);

    let fmt = time::format_description::parse("[year]-[month]-[day] [hour]:[minute]").unwrap();
    dt.format(&fmt)
        .unwrap_or_else(|_| "1970-01-01 00:00".to_owned())
}
