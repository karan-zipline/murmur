mod chat;
mod client;
mod core;
mod daemon_client;
mod editor;
mod markdown;
mod runtime;
mod view;

use fugue_core::paths::FuguePaths;

pub async fn run(paths: &FuguePaths) -> anyhow::Result<()> {
    let client = daemon_client::DaemonTuiClient::new(paths.clone());
    runtime::run(&client).await
}
