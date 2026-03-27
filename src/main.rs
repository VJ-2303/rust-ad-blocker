mod admin;
mod blocklist;
mod config;
mod dns;
mod error;
mod metrics;
mod server;

use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::{blocklist::Blocklist, dns::cache::Cache, error::Result};

#[tokio::main]
async fn main() -> Result<()> {
    let config = config::Config::load("config.toml")?;

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(&config.log_level))
        .with_target(false)
        .init();

    info!("Loading blocklist...");

    let blocklist = Blocklist::load(&config.blocklist_path)?;

    info!(
        domain_count = blocklist.len().await,
        "Successfully loaded blocklist"
    );

    info!(
        listen_addr = %config.listen_addr,
        upstream = %config.upstream_dns,
        "Starting RustHole DNS Server"
    );

    let cache = Cache::new();

    server::run(&config.listen_addr, &config.upstream_dns, blocklist, cache).await?;
    Ok(())
}
