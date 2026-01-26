mod chat;
mod client;
mod core;
mod daemon_client;
mod editor;
mod markdown;
mod runtime;
mod view;

use std::sync::Arc;

use murmur_core::paths::MurmurPaths;

pub async fn run(paths: &MurmurPaths) -> anyhow::Result<()> {
    let client: Arc<dyn client::TuiClient> =
        Arc::new(daemon_client::DaemonTuiClient::new(paths.clone()));
    runtime::run(client).await
}
