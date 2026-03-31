mod admin;
mod blocklist;
mod config;
mod dns;
mod error;
mod metrics;
mod server;

use std::{sync::Arc, time::Duration};
use tokio::net::UdpSocket;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use crate::{
    admin::state::AppState,
    blocklist::{Blocklist, loader::fetch_remote_blocklist},
    dns::{cache::Cache, upstream::UpstreamMultiplexer},
    error::Result,
    metrics::Metrics,
    server::ServerState,
};

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "config.toml".to_string());
    let config = config::Config::load(&config_path)?;

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(&config.log_level))
        .with_target(false)
        .init();

    info!("Loading blocklist...");

    let blocklist = Arc::new(Blocklist::load(&config.blocklist_path)?);
    let cache = Cache::new();
    let metrics = Arc::new(Metrics::default());
    let socket = Arc::new(UdpSocket::bind(&config.listen_addr).await?);

    let upstream_a = Arc::new(UdpSocket::bind("0.0.0.0:0").await?);
    let upstream_b = Arc::new(UdpSocket::bind("0.0.0.0:0").await?);

    let multiplexer = UpstreamMultiplexer::new(upstream_a, upstream_b);
    let upstream_addr = Arc::new(config.upstream_dns.clone());

    let state: ServerState = ServerState {
        socket,
        blocklist,
        cache,
        metrics,
        multiplexer,
        upstream_addr,
    };

    info!(
        domain_count = state.blocklist.len(),
        "Successfully loaded blocklist"
    );

    info!(
        listen_addr = %config.listen_addr,
        upstream = %config.upstream_dns,
        "Starting Adblocker DNS Server"
    );

    let task_state = state.clone();

    tokio::spawn(async move {
        if let Err(e) = server::run(task_state).await {
            tracing::error!("DNS server crashed: {}", e);
        }
    });

    let task_blocklist = state.blocklist.clone();

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
                    task_blocklist.update_list(new_list);
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

    let task_cache = state.cache.clone();

    tokio::spawn(async move {
        loop {
            task_cache.clean_expired();
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });

    tracing::info!(admin_addr = %config.admin_addr, "Starting Admin Web API");

    let listener = tokio::net::TcpListener::bind(&config.admin_addr).await?;

    let app_state = AppState {
        metrics: state.metrics.clone(),
        blocklist: state.blocklist.clone(),
    };

    axum::serve(listener, admin::routes::app(app_state)).await?;

    Ok(())
}
