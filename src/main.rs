mod admin;
mod blocklist;
mod config;
mod dns;
mod error;
mod metrics;
mod server;

use std::{sync::Arc, time::Duration};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use crate::{
    admin::state::AppState,
    blocklist::{Blocklist, loader::fetch_remote_blocklist},
    dns::cache::Cache,
    error::Result,
    metrics::Metrics,
};

#[tokio::main]
async fn main() -> Result<()> {
    let config = config::Config::load("config.toml")?;

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(&config.log_level))
        .with_target(false)
        .init();

    info!("Loading blocklist...");

    let blocklist = Arc::new(Blocklist::load(&config.blocklist_path)?);
    let cache = Cache::new();
    let metrics = Arc::new(Metrics::default());

    info!(
        domain_count = blocklist.len().await,
        "Successfully loaded blocklist"
    );

    info!(
        listen_addr = %config.listen_addr,
        upstream = %config.upstream_dns,
        "Starting RustHoldatae DNS Server"
    );

    let task_listen = config.listen_addr.clone();
    let task_upstream = config.upstream_dns.clone();
    let task_blocklist = blocklist.clone();
    let task_cache = cache.clone();
    let task_metrics = metrics.clone();

    tokio::spawn(async move {
        if let Err(e) = server::run(
            &task_listen,
            &task_upstream,
            task_blocklist,
            task_cache,
            task_metrics,
        )
        .await
        {
            tracing::error!("DNS server crashed: {}", e);
        }
    });

    let task_blocklist = blocklist.clone();

    tokio::spawn(async move {
        loop {
            let mut success = false;
            for attempt in 1..=3 {
                if let Ok(new_list) = fetch_remote_blocklist(
                    "https://raw.githubusercontent.com/StevenBlack/hosts/master/hosts",
                )
                .await
                {
                    success = true;
                    task_blocklist.update_list(new_list).await;
                    break;
                }
                warn!("Attempt {} failed, retrying in 10s...", attempt);
                tokio::time::sleep(Duration::from_secs(10)).await;
            }

            if success {
                info!("Blocklist fetched from the internet successfully");
            } else {
                error!("Failed to fetch blocklist from internet");
            }

            tokio::time::sleep(Duration::from_secs(86400)).await;
        }
    });

    let task_cache = cache.clone();

    tokio::spawn(async move {
        loop {
            task_cache.clean_expired().await;
            tokio::time::sleep(Duration::from_secs(300)).await;
        }
    });

    tracing::info!("Starting Admin Web API on 0.0.0.0:8080");

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;

    let app_state = AppState {
        metrics: metrics.clone(),
        blocklist: blocklist.clone(),
    };

    axum::serve(listener, admin::routes::app(app_state)).await?;

    Ok(())
}
