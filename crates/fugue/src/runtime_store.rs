use anyhow::Context as _;
use fugue_core::paths::FuguePaths;

pub async fn save_agents(
    paths: &FuguePaths,
    agents_json: &serde_json::Value,
) -> anyhow::Result<()> {
    tokio::fs::create_dir_all(&paths.runtime_dir)
        .await
        .with_context(|| format!("create runtime dir: {}", paths.runtime_dir.display()))?;

    let tmp = paths.runtime_dir.join("agents.json.tmp");
    let dest = paths.runtime_dir.join("agents.json");

    let data = serde_json::to_vec_pretty(agents_json).context("serialize agents runtime json")?;

    tokio::fs::write(&tmp, &data)
        .await
        .with_context(|| format!("write {}", tmp.display()))?;
    tokio::fs::rename(&tmp, &dest)
        .await
        .with_context(|| format!("rename {} -> {}", tmp.display(), dest.display()))?;

    Ok(())
}
