use std::env;
use std::fs::File;
use std::io::{self, BufRead as _, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context as _};
use fugue_core::usage::{current_billing_window, parse_usage_entry, BillingWindow, Usage};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

pub struct ClaudeUsageWindow {
    pub now: OffsetDateTime,
    pub window: BillingWindow,
    pub usage: Usage,
}

pub fn get_current_billing_window_with_usage() -> anyhow::Result<ClaudeUsageWindow> {
    let now = OffsetDateTime::now_utc();
    let projects_dir = claude_projects_dir()?;

    if !projects_dir.exists() {
        return Ok(ClaudeUsageWindow {
            now,
            window: current_billing_window(now, vec![]),
            usage: Usage::default(),
        });
    }

    let session_files = find_jsonl_session_files(&projects_dir)?;
    let mut timestamps = Vec::new();
    for file in &session_files {
        collect_timestamps(file, &mut timestamps)?;
    }

    let window = current_billing_window(now, timestamps);

    let mut usage = Usage::default();
    for file in &session_files {
        collect_usage_in_window(file, window, &mut usage)?;
    }

    Ok(ClaudeUsageWindow { now, window, usage })
}

pub fn format_duration(d: Duration) -> String {
    let seconds = d.whole_seconds().max(0);
    let total_minutes = (seconds + 30) / 60;
    let hours = total_minutes / 60;
    let minutes = total_minutes % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

pub fn format_rfc3339(ts: OffsetDateTime) -> anyhow::Result<String> {
    ts.format(&Rfc3339).context("format rfc3339 timestamp")
}

fn claude_projects_dir() -> anyhow::Result<PathBuf> {
    let home = env::var_os("HOME").ok_or_else(|| anyhow!("HOME is not set"))?;
    Ok(PathBuf::from(home).join(".claude").join("projects"))
}

fn find_jsonl_session_files(projects_dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let projects = match std::fs::read_dir(projects_dir) {
        Ok(v) => v,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(files),
        Err(err) => return Err(err).context("read .claude/projects dir"),
    };

    for entry in projects {
        let entry = entry.context("read .claude project entry")?;
        let ty = entry
            .file_type()
            .context("read .claude project entry type")?;
        if !ty.is_dir() {
            continue;
        }

        let sessions = std::fs::read_dir(entry.path())
            .with_context(|| format!("read .claude project dir: {}", entry.path().display()))?;
        for sess in sessions {
            let sess = sess.context("read .claude session entry")?;
            let ty = sess
                .file_type()
                .context("read .claude session entry type")?;
            if !ty.is_file() {
                continue;
            }

            let path = sess.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            files.push(path);
        }
    }

    Ok(files)
}

fn collect_timestamps(path: &Path, out: &mut Vec<OffsetDateTime>) -> anyhow::Result<()> {
    let file =
        File::open(path).with_context(|| format!("open session file: {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader
            .read_line(&mut line)
            .with_context(|| format!("read session file: {}", path.display()))?;
        if n == 0 {
            break;
        }

        if let Some(entry) = parse_usage_entry(line.trim_end()) {
            out.push(entry.timestamp);
        }
    }
    Ok(())
}

fn collect_usage_in_window(
    path: &Path,
    window: BillingWindow,
    usage: &mut Usage,
) -> anyhow::Result<()> {
    let file =
        File::open(path).with_context(|| format!("open session file: {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader
            .read_line(&mut line)
            .with_context(|| format!("read session file: {}", path.display()))?;
        if n == 0 {
            break;
        }

        let Some(entry) = parse_usage_entry(line.trim_end()) else {
            continue;
        };
        if window.contains(entry.timestamp) {
            usage.add(entry.usage);
        }
    }
    Ok(())
}
