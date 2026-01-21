use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context as _, Result};
use fugue_core::config::ConfigFile;
use fugue_core::paths::FuguePaths;

pub async fn load(paths: &FuguePaths) -> Result<ConfigFile> {
    let path = &paths.config_file;
    match tokio::fs::read_to_string(path).await {
        Ok(s) => {
            let cfg: ConfigFile = toml::from_str(&s).context("parse config.toml")?;
            cfg.validate().context("validate config.toml")?;
            Ok(cfg)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(ConfigFile::default()),
        Err(err) => Err(err).with_context(|| format!("read config: {}", path.display())),
    }
}

pub async fn save(paths: &FuguePaths, config: &ConfigFile) -> Result<()> {
    config.validate().context("validate config")?;

    let s = toml::to_string(config).context("serialize config")?;
    write_atomic_string(&paths.config_file, &s).await
}

async fn write_atomic_string(path: &Path, contents: &str) -> Result<()> {
    let parent = path
        .parent()
        .context("config path has no parent directory")?;
    tokio::fs::create_dir_all(parent)
        .await
        .with_context(|| format!("create config dir: {}", parent.display()))?;

    let tmp = tmp_path(path);
    tokio::fs::write(&tmp, contents)
        .await
        .with_context(|| format!("write temp config: {}", tmp.display()))?;

    tokio::fs::rename(&tmp, path)
        .await
        .with_context(|| format!("rename {} -> {}", tmp.display(), path.display()))?;

    Ok(())
}

fn tmp_path(path: &Path) -> PathBuf {
    let file_name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("config.toml");
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();

    path.with_file_name(format!(".{file_name}.{nonce}.tmp"))
}
