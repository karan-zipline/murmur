use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ClaimError {
    #[error("issue already claimed: {project}/{issue_id} by {agent_id}")]
    AlreadyClaimed {
        project: String,
        issue_id: String,
        agent_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimEntry {
    pub project: String,
    pub issue_id: String,
    pub agent_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClaimRegistry {
    claims: BTreeMap<(String, String), String>,
}

impl ClaimRegistry {
    pub fn is_claimed(&self, project: &str, issue_id: &str) -> bool {
        self.claims
            .contains_key(&(project.to_owned(), issue_id.to_owned()))
    }

    pub fn agent_for(&self, project: &str, issue_id: &str) -> Option<&str> {
        self.claims
            .get(&(project.to_owned(), issue_id.to_owned()))
            .map(|v| v.as_str())
    }

    pub fn claim(&self, project: &str, issue_id: &str, agent_id: &str) -> Result<Self, ClaimError> {
        let key = (project.to_owned(), issue_id.to_owned());
        if let Some(existing) = self.claims.get(&key) {
            return Err(ClaimError::AlreadyClaimed {
                project: project.to_owned(),
                issue_id: issue_id.to_owned(),
                agent_id: existing.clone(),
            });
        }

        let mut next = self.clone();
        next.claims.insert(key, agent_id.to_owned());
        Ok(next)
    }

    pub fn release(&self, project: &str, issue_id: &str) -> Self {
        let mut next = self.clone();
        next.claims
            .remove(&(project.to_owned(), issue_id.to_owned()));
        next
    }

    pub fn release_by_agent(&self, agent_id: &str) -> Self {
        let mut next = BTreeMap::new();
        for ((project, issue_id), claimed_by) in &self.claims {
            if claimed_by == agent_id {
                continue;
            }
            next.insert((project.clone(), issue_id.clone()), claimed_by.clone());
        }
        Self { claims: next }
    }

    pub fn list(&self) -> Vec<ClaimEntry> {
        self.claims
            .iter()
            .map(|((project, issue_id), agent_id)| ClaimEntry {
                project: project.clone(),
                issue_id: issue_id.clone(),
                agent_id: agent_id.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_collides_when_already_claimed() {
        let claims = ClaimRegistry::default()
            .claim("demo", "ISSUE-1", "a-1")
            .unwrap();

        let err = claims.claim("demo", "ISSUE-1", "a-2").unwrap_err();
        match err {
            ClaimError::AlreadyClaimed {
                project,
                issue_id,
                agent_id,
            } => {
                assert_eq!(project, "demo");
                assert_eq!(issue_id, "ISSUE-1");
                assert_eq!(agent_id, "a-1");
            }
        }
    }

    #[test]
    fn release_is_idempotent() {
        let claims = ClaimRegistry::default()
            .claim("demo", "ISSUE-1", "a-1")
            .unwrap();
        let released = claims.release("demo", "ISSUE-1");
        assert!(!released.is_claimed("demo", "ISSUE-1"));

        let released_again = released.release("demo", "ISSUE-1");
        assert_eq!(released_again, released);
    }

    #[test]
    fn release_by_agent_removes_only_matching_claims() {
        let claims = ClaimRegistry::default()
            .claim("demo", "ISSUE-1", "a-1")
            .unwrap()
            .claim("demo", "ISSUE-2", "a-2")
            .unwrap()
            .claim("other", "X-1", "a-1")
            .unwrap();

        let next = claims.release_by_agent("a-1");
        assert!(!next.is_claimed("demo", "ISSUE-1"));
        assert!(!next.is_claimed("other", "X-1"));
        assert!(next.is_claimed("demo", "ISSUE-2"));
        assert_eq!(next.agent_for("demo", "ISSUE-2"), Some("a-2"));
    }
}
