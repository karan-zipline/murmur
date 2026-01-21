use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DedupEntry {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    pub created_at_ms: u64,
}

pub struct DedupStore {
    path: PathBuf,
    entries: BTreeMap<String, DedupEntry>,
    max_age: Duration,
    max_entries: usize,
}

impl DedupStore {
    pub const DEFAULT_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);
    pub const DEFAULT_MAX_ENTRIES: usize = 10_000;

    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            entries: BTreeMap::new(),
            max_age: Self::DEFAULT_MAX_AGE,
            max_entries: Self::DEFAULT_MAX_ENTRIES,
        }
    }

    pub async fn load(path: PathBuf, now_ms: u64) -> anyhow::Result<Self> {
        let mut store = Self::new(path);

        let data = match tokio::fs::read(&store.path).await {
            Ok(v) => v,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(store);
            }
            Err(err) => return Err(err).with_context(|| format!("read {}", store.path.display())),
        };

        if data.is_empty() {
            return Ok(store);
        }

        let entries: Vec<DedupEntry> =
            serde_json::from_slice(&data).context("parse dedup store json")?;

        store.entries = entries
            .into_iter()
            .map(|e| (e.id.clone(), e))
            .collect::<BTreeMap<_, _>>();

        store.cleanup(now_ms);
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn mark(&mut self, id: &str, project: Option<&str>, now_ms: u64) -> bool {
        if id.trim().is_empty() {
            return false;
        }

        if self.entries.contains_key(id) {
            return false;
        }

        self.entries.insert(
            id.to_owned(),
            DedupEntry {
                id: id.to_owned(),
                project: project
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned),
                created_at_ms: now_ms,
            },
        );

        if self.entries.len() > self.max_entries {
            self.cleanup(now_ms);
        }

        true
    }

    pub fn cleanup(&mut self, now_ms: u64) -> usize {
        let before = self.entries.len();

        let cutoff_ms = now_ms.saturating_sub(self.max_age.as_millis() as u64);
        self.entries.retain(|_, e| e.created_at_ms >= cutoff_ms);

        if self.entries.len() > self.max_entries {
            let mut by_age = self
                .entries
                .iter()
                .map(|(id, e)| (id.clone(), e.created_at_ms))
                .collect::<Vec<_>>();
            by_age.sort_by_key(|(_, ms)| *ms);

            let excess = self.entries.len() - self.max_entries;
            for (id, _) in by_age.into_iter().take(excess) {
                self.entries.remove(&id);
            }
        }

        before.saturating_sub(self.entries.len())
    }

    pub fn entries_snapshot(&self) -> Vec<DedupEntry> {
        self.entries.values().cloned().collect()
    }

    pub async fn save_snapshot(path: &Path, entries: &[DedupEntry]) -> anyhow::Result<()> {
        let Some(parent) = path.parent() else {
            return Err(anyhow::anyhow!(
                "invalid dedup store path: {}",
                path.display()
            ));
        };

        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("create {}", parent.display()))?;

        let data = serde_json::to_vec_pretty(entries).context("serialize dedup store json")?;

        let tmp = path.with_extension("json.tmp");
        tokio::fs::write(&tmp, &data)
            .await
            .with_context(|| format!("write {}", tmp.display()))?;
        tokio::fs::rename(&tmp, path)
            .await
            .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn mark_dedupes_ids() {
        let mut store = DedupStore::new(PathBuf::from("/tmp/dedup.json"));
        assert!(store.mark("id-1", Some("demo"), 1000));
        assert!(!store.mark("id-1", Some("demo"), 1001));
    }

    #[test]
    fn cleanup_removes_old_entries() {
        let mut store = DedupStore::new(PathBuf::from("/tmp/dedup.json"));
        store.max_age = Duration::from_secs(1);
        store.mark("id-1", None, 1000);
        store.mark("id-2", None, 2500);
        let removed = store.cleanup(3000);
        assert_eq!(removed, 1);
        assert!(store.entries.contains_key("id-2"));
        assert!(!store.entries.contains_key("id-1"));
    }

    #[tokio::test]
    async fn save_and_load_round_trip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("runtime").join("dedup.json");

        let mut store = DedupStore::new(path.clone());
        store.mark("id-1", Some("demo"), 1000);
        DedupStore::save_snapshot(&path, &store.entries_snapshot())
            .await
            .unwrap();

        let loaded = DedupStore::load(path.clone(), 1000).await.unwrap();
        assert!(loaded.entries.contains_key("id-1"));
    }
}
