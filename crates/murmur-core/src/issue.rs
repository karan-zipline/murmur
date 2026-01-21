use std::collections::{BTreeSet, VecDeque};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Open,
    Closed,
    Blocked,
}

impl Status {
    pub fn parse(s: &str) -> Result<Self, ParseStatusError> {
        match s.trim().to_ascii_lowercase().as_str() {
            "open" => Ok(Status::Open),
            "closed" => Ok(Status::Closed),
            "blocked" => Ok(Status::Blocked),
            other => Err(ParseStatusError::UnknownStatus {
                value: other.to_owned(),
            }),
        }
    }
}

#[derive(Debug, Error)]
pub enum ParseStatusError {
    #[error("unknown status: {value}")]
    UnknownStatus { value: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: Status,
    pub priority: i32,
    #[serde(rename = "type")]
    pub issue_type: String,
    pub dependencies: Vec<String>,
    pub labels: Vec<String>,
    pub links: Vec<String>,
    pub created_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateParams {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "type")]
    pub issue_type: String,
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub links: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UpdateParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
    #[serde(default, rename = "type", skip_serializing_if = "Option::is_none")]
    pub issue_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub links: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ListFilter {
    #[serde(default)]
    pub status: Vec<Status>,
    #[serde(default)]
    pub labels: Vec<String>,
}

impl ListFilter {
    pub fn matches(&self, issue: &Issue) -> bool {
        if !self.status.is_empty() && !self.status.contains(&issue.status) {
            return false;
        }

        if self.labels.is_empty() {
            return true;
        }

        let set: BTreeSet<&str> = issue.labels.iter().map(String::as_str).collect();
        self.labels.iter().all(|l| set.contains(l.as_str()))
    }
}

pub fn compute_ready_issues(open_issues: Vec<Issue>) -> Vec<Issue> {
    let open_ids: BTreeSet<String> = open_issues.iter().map(|i| i.id.clone()).collect();
    open_issues
        .into_iter()
        .filter(|iss| !iss.dependencies.iter().any(|d| open_ids.contains(d)))
        .collect()
}

#[derive(Debug, Error)]
pub enum TkError {
    #[error("empty file")]
    EmptyFile,
    #[error("missing opening frontmatter delimiter")]
    MissingOpeningDelimiter,
    #[error("missing closing frontmatter delimiter")]
    MissingClosingDelimiter,
    #[error("parse frontmatter: {0}")]
    Frontmatter(String),
    #[error("parse created timestamp: {0}")]
    CreatedTimestamp(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TkFrontmatter {
    #[serde(default)]
    id: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    deps: Vec<String>,
    #[serde(default)]
    links: Vec<String>,
    #[serde(default)]
    created: Option<String>,
    #[serde(default, rename = "type")]
    issue_type: String,
    #[serde(default)]
    priority: i32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    labels: Vec<String>,
}

pub fn tk_split_frontmatter(input: &str) -> Result<(String, String), TkError> {
    let mut lines = input.lines();

    let first = lines.next().ok_or(TkError::EmptyFile)?;
    if first.trim() != "---" {
        return Err(TkError::MissingOpeningDelimiter);
    }

    let mut fm_lines = Vec::new();
    let mut found_close = false;
    for line in lines.by_ref() {
        if line.trim() == "---" {
            found_close = true;
            break;
        }
        fm_lines.push(line);
    }
    if !found_close {
        return Err(TkError::MissingClosingDelimiter);
    }

    let fm = fm_lines.join("\n");
    let body = lines.collect::<Vec<_>>().join("\n");
    Ok((fm, body))
}

pub fn tk_parse_body(body: &str) -> (String, String) {
    let lines: Vec<&str> = body.lines().collect();
    let mut start = 0;
    while start < lines.len() && lines[start].trim().is_empty() {
        start += 1;
    }

    let mut title = String::new();
    if start < lines.len() {
        if let Some(rest) = lines[start].strip_prefix("# ") {
            title = rest.to_owned();
            start += 1;
        }
    }

    let description = if start < lines.len() {
        lines[start..].join("\n").trim().to_owned()
    } else {
        String::new()
    };

    (title, description)
}

pub fn tk_parse_issue(input: &str) -> Result<Issue, TkError> {
    let (fm, body) = tk_split_frontmatter(input)?;

    let meta: TkFrontmatter =
        serde_yaml::from_str(&fm).map_err(|e| TkError::Frontmatter(e.to_string()))?;

    let (title, description) = tk_parse_body(&body);

    let status = if meta.status.trim().is_empty() {
        Status::Open
    } else {
        Status::parse(&meta.status).map_err(|e| TkError::Frontmatter(e.to_string()))?
    };

    let created_at_ms = meta
        .created
        .as_deref()
        .and_then(|s| parse_rfc3339_ms(s).ok())
        .unwrap_or(0);

    Ok(Issue {
        id: meta.id,
        title,
        description,
        status,
        priority: meta.priority,
        issue_type: meta.issue_type,
        dependencies: meta.deps,
        labels: meta.labels,
        links: meta.links,
        created_at_ms,
    })
}

pub fn tk_format_issue(issue: &Issue) -> Result<String, TkError> {
    let created = if issue.created_at_ms == 0 {
        None
    } else {
        Some(format_rfc3339_ms(issue.created_at_ms))
    };

    let meta = TkFrontmatter {
        id: issue.id.clone(),
        status: format!("{:?}", issue.status).to_ascii_lowercase(),
        deps: issue.dependencies.clone(),
        links: issue.links.clone(),
        created,
        issue_type: issue.issue_type.clone(),
        priority: issue.priority,
        labels: issue.labels.clone(),
    };

    let fm = serde_yaml::to_string(&meta).map_err(|e| TkError::Frontmatter(e.to_string()))?;

    let mut out = String::new();
    out.push_str("---\n");
    out.push_str(fm.trim_end());
    out.push('\n');
    out.push_str("---\n");
    if !issue.title.is_empty() {
        out.push_str("# ");
        out.push_str(&issue.title);
        out.push_str("\n\n");
    }
    if !issue.description.is_empty() {
        out.push_str(&issue.description);
        out.push('\n');
    }
    Ok(out)
}

pub fn tk_upsert_comment(body: &str, comment: &str) -> String {
    let body = body.replace("\r\n", "\n");
    let comment = comment.trim();

    let mut lines: VecDeque<&str> = body.lines().collect();
    if lines.is_empty() && body.is_empty() {
        return format!("## Comments\n\n{comment}\n");
    }

    let mut before = Vec::new();
    let mut found = false;
    while let Some(line) = lines.pop_front() {
        if line.trim() == "## Comments" {
            found = true;
            break;
        }
        before.push(line);
    }

    if !found {
        let trimmed = body.trim_end_matches(&['\n', '\t', ' '][..]);
        if trimmed.is_empty() {
            return format!("## Comments\n\n{comment}\n");
        }
        return format!("{trimmed}\n\n## Comments\n\n{comment}\n");
    }

    let mut existing = Vec::new();
    let mut after = Vec::new();
    let mut in_after = false;
    for line in lines {
        if !in_after && line.starts_with("## ") {
            in_after = true;
        }
        if in_after {
            after.push(line);
        } else {
            existing.push(line);
        }
    }

    let before = trim_right_lines(before);
    let existing = trim_both_lines(existing);
    let after = trim_both_lines(after);

    let mut out = String::new();
    if !before.is_empty() {
        out.push_str(&before);
        out.push_str("\n\n");
    }

    out.push_str("## Comments\n\n");

    if !existing.is_empty() {
        out.push_str(&existing);
        out.push_str("\n\n");
    }

    out.push_str(comment);

    if !after.is_empty() {
        out.push_str("\n\n");
        out.push_str(&after);
    }

    out.push('\n');
    out
}

pub fn upsert_plan_section(body: &str, plan_content: &str) -> String {
    let body = body.replace("\r\n", "\n");
    let plan_content = plan_content.replace("\r\n", "\n");
    let plan_content = plan_content.trim();

    if body.trim().is_empty() {
        return format!("## Plan\n\n{plan_content}\n");
    }

    let mut lines: VecDeque<&str> = body.lines().collect();

    let mut before = Vec::new();
    let mut found = false;
    while let Some(line) = lines.pop_front() {
        if line.trim() == "## Plan" {
            found = true;
            break;
        }
        before.push(line);
    }

    if !found {
        let trimmed = body.trim_end_matches(&['\n', '\t', ' '][..]);
        if trimmed.is_empty() {
            return format!("## Plan\n\n{plan_content}\n");
        }
        return format!("{trimmed}\n\n## Plan\n\n{plan_content}\n");
    }

    let mut after = Vec::new();
    let mut in_after = false;
    let mut existing = Vec::new();
    for line in lines {
        if !in_after && line.starts_with("## ") {
            in_after = true;
        }
        if in_after {
            after.push(line);
        } else {
            existing.push(line);
        }
    }

    let before = trim_right_lines(before);
    let after = trim_both_lines(after);

    let mut out = String::new();
    if !before.is_empty() {
        out.push_str(&before);
        out.push_str("\n\n");
    }

    out.push_str("## Plan\n\n");
    out.push_str(plan_content);

    if !after.is_empty() {
        out.push_str("\n\n");
        out.push_str(&after);
    }
    out.push('\n');
    out
}

fn trim_right_lines(lines: Vec<&str>) -> String {
    let mut out = lines.join("\n");
    while out.ends_with(['\n', '\t', ' ']) {
        out.pop();
    }
    out
}

fn trim_both_lines(lines: Vec<&str>) -> String {
    let mut out = lines.join("\n");
    out = out.trim_matches(['\n', '\t', ' ']).to_owned();
    out
}

fn parse_rfc3339_ms(s: &str) -> Result<u64, TkError> {
    let dt = time::OffsetDateTime::parse(s.trim(), &time::format_description::well_known::Rfc3339)
        .map_err(|e| TkError::CreatedTimestamp(e.to_string()))?;
    Ok(dt.unix_timestamp_nanos() as u64 / 1_000_000)
}

fn format_rfc3339_ms(ms: u64) -> String {
    let secs = (ms / 1000) as i64;
    let nanos = ((ms % 1000) * 1_000_000) as u32;
    let dt = time::OffsetDateTime::from_unix_timestamp(secs)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
        .replace_nanosecond(nanos)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    dt.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_parses_expected_values() {
        assert_eq!(Status::parse("open").unwrap(), Status::Open);
        assert_eq!(Status::parse("CLOSED").unwrap(), Status::Closed);
        assert_eq!(Status::parse("blocked").unwrap(), Status::Blocked);
        assert!(Status::parse("wat").is_err());
    }

    #[test]
    fn list_filter_matches_labels_and_status() {
        let issue = Issue {
            id: "a".to_owned(),
            title: "t".to_owned(),
            description: "".to_owned(),
            status: Status::Open,
            priority: 0,
            issue_type: "task".to_owned(),
            dependencies: vec![],
            labels: vec!["one".to_owned(), "two".to_owned()],
            links: vec![],
            created_at_ms: 0,
        };

        assert!(ListFilter::default().matches(&issue));
        assert!(ListFilter {
            status: vec![Status::Open],
            labels: vec![]
        }
        .matches(&issue));
        assert!(!ListFilter {
            status: vec![Status::Closed],
            labels: vec![]
        }
        .matches(&issue));
        assert!(ListFilter {
            status: vec![Status::Open],
            labels: vec!["one".to_owned()]
        }
        .matches(&issue));
        assert!(!ListFilter {
            status: vec![Status::Open],
            labels: vec!["missing".to_owned()]
        }
        .matches(&issue));
    }

    #[test]
    fn tk_split_frontmatter_errors_on_missing_delimiters() {
        assert!(matches!(
            tk_split_frontmatter("id: x\n---\n# t"),
            Err(TkError::MissingOpeningDelimiter)
        ));
        assert!(matches!(
            tk_split_frontmatter("---\nid: x\n# t"),
            Err(TkError::MissingClosingDelimiter)
        ));
    }

    #[test]
    fn tk_upsert_comment_matches_fab_cases() {
        let got = tk_upsert_comment("", "First comment");
        assert_eq!(got, "## Comments\n\nFirst comment\n");

        let got = tk_upsert_comment("## Summary\n\nThis is the summary.", "A comment");
        assert_eq!(
            got,
            "## Summary\n\nThis is the summary.\n\n## Comments\n\nA comment\n"
        );

        let got = tk_upsert_comment(
            "## Summary\r\n\r\nText.\r\n\r\n## Comments\r\n\r\nOld.",
            "New.",
        );
        assert_eq!(got, "## Summary\n\nText.\n\n## Comments\n\nOld.\n\nNew.\n");
    }

    #[test]
    fn upsert_plan_section_matches_fab_cases() {
        let got = upsert_plan_section("", "- [ ] Step 1\n- [ ] Step 2");
        assert_eq!(got, "## Plan\n\n- [ ] Step 1\n- [ ] Step 2\n");

        let got = upsert_plan_section(
            "## Summary\n\nThis is the summary.",
            "- [ ] Step 1\n- [ ] Step 2",
        );
        assert_eq!(
            got,
            "## Summary\n\nThis is the summary.\n\n## Plan\n\n- [ ] Step 1\n- [ ] Step 2\n"
        );

        let got = upsert_plan_section(
            "## Summary\n\nThis is the summary.\n\n## Plan\n\n- [ ] Old step 1\n- [ ] Old step 2\n",
            "- [ ] New step 1\n- [ ] New step 2",
        );
        assert_eq!(
            got,
            "## Summary\n\nThis is the summary.\n\n## Plan\n\n- [ ] New step 1\n- [ ] New step 2\n"
        );

        let got = upsert_plan_section(
            "## Summary\n\nSummary text.\n\n## Plan\n\n- [ ] Old step\n\n## Notes\n\nSome notes.",
            "- [ ] New step",
        );
        assert_eq!(
            got,
            "## Summary\n\nSummary text.\n\n## Plan\n\n- [ ] New step\n\n## Notes\n\nSome notes.\n"
        );

        let got = upsert_plan_section(
            "## Plan\n\n- [ ] Old step\n\n## Summary\n\nSummary text.",
            "- [ ] New step",
        );
        assert_eq!(
            got,
            "## Plan\n\n- [ ] New step\n\n## Summary\n\nSummary text.\n"
        );

        let got = upsert_plan_section(
            "## Summary\n\nSummary text.\n\n## Plan\n\n- [ ] Old step",
            "- [ ] New step",
        );
        assert_eq!(
            got,
            "## Summary\n\nSummary text.\n\n## Plan\n\n- [ ] New step\n"
        );

        let got = upsert_plan_section(
            "Introduction text.\n\n## Summary\n\nSummary.\n\n## Plan\n\nOld plan.\n\n## Testing\n\nTest instructions.\n\nFinal notes.",
            "New plan content.",
        );
        assert_eq!(
            got,
            "Introduction text.\n\n## Summary\n\nSummary.\n\n## Plan\n\nNew plan content.\n\n## Testing\n\nTest instructions.\n\nFinal notes.\n"
        );

        let got = upsert_plan_section("## Summary\r\n\r\nText.\r\n\r\n## Plan\r\n\r\nOld.", "New.");
        assert_eq!(got, "## Summary\n\nText.\n\n## Plan\n\nNew.\n");

        let got = upsert_plan_section("## Summary\n\nText.", "  \n\n- [ ] Step 1\n\n  ");
        assert_eq!(got, "## Summary\n\nText.\n\n## Plan\n\n- [ ] Step 1\n");

        let got = upsert_plan_section("## Plan\n\n- [ ] Step 1\n", "- [ ] Step 1");
        assert_eq!(got, "## Plan\n\n- [ ] Step 1\n");

        let got = upsert_plan_section("   \n\n  ", "- [ ] Step 1");
        assert_eq!(got, "## Plan\n\n- [ ] Step 1\n");

        let got = upsert_plan_section(
            "## Summary\n\nText.\n\n## Plan\n\n## Notes\n\nNotes.",
            "- [ ] New step",
        );
        assert_eq!(
            got,
            "## Summary\n\nText.\n\n## Plan\n\n- [ ] New step\n\n## Notes\n\nNotes.\n"
        );

        let body = "## Summary\n\nThis is a test.\n";
        let plan_content = "- [ ] Step 1\n- [ ] Step 2";
        let result1 = upsert_plan_section(body, plan_content);
        let result2 = upsert_plan_section(&result1, plan_content);
        assert_eq!(result1, result2);
    }

    #[test]
    fn tk_parse_and_format_round_trip() {
        let issue = Issue {
            id: "issue-123".to_owned(),
            title: "Add user authentication".to_owned(),
            description: "Implement OAuth2 login flow.".to_owned(),
            status: Status::Open,
            priority: 2,
            issue_type: "feature".to_owned(),
            dependencies: vec!["issue-100".to_owned()],
            labels: vec!["backend".to_owned(), "api".to_owned()],
            links: vec!["https://example.com".to_owned()],
            created_at_ms: 1_705_316_800_000,
        };

        let formatted = tk_format_issue(&issue).unwrap();
        let parsed = tk_parse_issue(&formatted).unwrap();
        assert_eq!(parsed, issue);
    }

    #[test]
    fn tk_parse_handles_multiline_description() {
        let input = r#"---
id: issue-1
status: open
---
# Title

First paragraph.

Second paragraph.

- Item 1
- Item 2
"#;
        let iss = tk_parse_issue(input).unwrap();
        assert_eq!(iss.title, "Title");
        assert_eq!(
            iss.description,
            "First paragraph.\n\nSecond paragraph.\n\n- Item 1\n- Item 2"
        );
    }
}
