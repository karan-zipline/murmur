use std::collections::BTreeSet;

use crate::claims::ClaimRegistry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnPlan {
    pub issue_ids: Vec<String>,
}

pub fn orchestrator_tick<'a, I>(
    project: &str,
    active_agents: usize,
    max_agents: usize,
    ready_issue_ids: I,
    claims: &ClaimRegistry,
) -> SpawnPlan
where
    I: IntoIterator<Item = &'a str>,
{
    let available = max_agents.saturating_sub(active_agents);
    if available == 0 {
        return SpawnPlan { issue_ids: vec![] };
    }

    let mut picked = BTreeSet::<&'a str>::new();
    let mut spawn = Vec::new();

    for issue_id in ready_issue_ids {
        if spawn.len() == available {
            break;
        }
        if !picked.insert(issue_id) {
            continue;
        }
        if claims.is_claimed(project, issue_id) {
            continue;
        }
        spawn.push(issue_id.to_owned());
    }

    SpawnPlan { issue_ids: spawn }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claims::ClaimRegistry;

    #[test]
    fn tick_does_not_spawn_when_at_capacity() {
        let claims = ClaimRegistry::default();
        let plan = orchestrator_tick("demo", 2, 2, vec!["A", "B"], &claims);
        assert!(plan.issue_ids.is_empty());
    }

    #[test]
    fn tick_spawns_up_to_available_slots() {
        let claims = ClaimRegistry::default();
        let plan = orchestrator_tick("demo", 1, 3, vec!["A", "B", "C"], &claims);
        assert_eq!(plan.issue_ids, vec!["A".to_owned(), "B".to_owned()]);
    }

    #[test]
    fn tick_skips_claimed_issues() {
        let claims = ClaimRegistry::default().claim("demo", "B", "a-1").unwrap();
        let plan = orchestrator_tick("demo", 0, 2, vec!["A", "B", "C"], &claims);
        assert_eq!(plan.issue_ids, vec!["A".to_owned(), "C".to_owned()]);
    }
}
