use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

pub const DEFAULT_COMMIT_LOG_SIZE: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitRecord {
    pub sha: String,
    pub branch: String,
    pub agent_id: String,
    pub issue_id: String,
    pub merged_at_ms: u64,
}

#[derive(Debug, Clone)]
pub struct CommitLog {
    max_size: usize,
    commits: VecDeque<CommitRecord>,
}

impl CommitLog {
    pub fn new(max_size: usize) -> Self {
        Self {
            max_size: max_size.max(1),
            commits: VecDeque::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.commits.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commits.is_empty()
    }

    pub fn add(&mut self, record: CommitRecord) {
        if self.commits.len() == self.max_size {
            self.commits.pop_front();
        }
        self.commits.push_back(record);
    }

    pub fn list(&self) -> Vec<CommitRecord> {
        self.commits.iter().cloned().collect()
    }

    pub fn list_recent(&self, count: usize) -> Vec<CommitRecord> {
        let count = count.min(self.commits.len());
        self.commits.iter().rev().take(count).cloned().collect()
    }
}

impl Default for CommitLog {
    fn default() -> Self {
        Self::new(DEFAULT_COMMIT_LOG_SIZE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_log_truncates_oldest() {
        let mut log = CommitLog::new(2);
        log.add(CommitRecord {
            sha: "a".to_owned(),
            branch: "b".to_owned(),
            agent_id: "a-1".to_owned(),
            issue_id: "ISSUE-1".to_owned(),
            merged_at_ms: 1,
        });
        log.add(CommitRecord {
            sha: "b".to_owned(),
            branch: "b".to_owned(),
            agent_id: "a-2".to_owned(),
            issue_id: "ISSUE-2".to_owned(),
            merged_at_ms: 2,
        });
        log.add(CommitRecord {
            sha: "c".to_owned(),
            branch: "b".to_owned(),
            agent_id: "a-3".to_owned(),
            issue_id: "ISSUE-3".to_owned(),
            merged_at_ms: 3,
        });

        let got = log.list();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].sha, "b");
        assert_eq!(got[1].sha, "c");
    }

    #[test]
    fn commit_log_lists_recent_newest_first() {
        let mut log = CommitLog::new(3);
        for i in 1..=3 {
            log.add(CommitRecord {
                sha: format!("{i}"),
                branch: "b".to_owned(),
                agent_id: format!("a-{i}"),
                issue_id: format!("ISSUE-{i}"),
                merged_at_ms: i as u64,
            });
        }

        let got = log.list_recent(2);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].sha, "3");
        assert_eq!(got[1].sha, "2");
    }
}
