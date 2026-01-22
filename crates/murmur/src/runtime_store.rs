use anyhow::Context as _;
use murmur_core::paths::MurmurPaths;
use murmur_protocol::AgentInfo;

pub async fn save_agents(
    paths: &MurmurPaths,
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

pub async fn load_agents(paths: &MurmurPaths) -> anyhow::Result<Vec<AgentInfo>> {
    let path = paths.runtime_dir.join("agents.json");
    if !path.exists() {
        return Ok(vec![]);
    }
    let data = tokio::fs::read_to_string(&path)
        .await
        .with_context(|| format!("read {}", path.display()))?;
    let infos: Vec<AgentInfo> =
        serde_json::from_str(&data).with_context(|| "parse agents.json")?;
    Ok(infos)
}
