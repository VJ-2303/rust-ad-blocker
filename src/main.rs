mod admin;
mod blocklist;
mod config;
mod dns;
mod error;
mod metrics;
mod server;

use std::sync::Arc;
use tracing::info;
use tracing_subscriber::EnvFilter;

use crate::{blocklist::Blocklist, dns::cache::Cache, error::Result, metrics::Metrics};

#[tokio::main]
async fn main() -> Result<()> {
    let config = config::Config::load("config.toml")?;

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(&config.log_level))
        .with_target(false)
        .init();

    info!("Loading blocklist...");

    let blocklist = Arc::new(Blocklist::load(&config.blocklist_path)?);

    info!(
        domain_count = blocklist.len().await,
        "Successfully loaded blocklist"
    );

    info!(
        listen_addr = %config.listen_addr,
        upstream = %config.upstream_dns,
        "Starting RustHoldatae DNS Server"
    );

    let cache = Cache::new();

    let metrics = Arc::new(Metrics::default());

    let task_listen = config.listen_addr.clone();
    let task_upstream = config.upstream_dns.clone();

    tokio::spawn(async move {
        if let Err(e) = server::run(
            &task_listen,
            &task_upstream,
            blocklist.clone(),
            cache.clone(),
            metrics.clone(),
        )
        .await
        {
            tracing::error!("DNS server crashed: {}", e);
        }
    });

    tracing::info!("Starting Admin Web API on 0.0.0.0:8080");

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;

    axum::serve(listener, admin::routes::app()).await?;

    Ok(())
}
