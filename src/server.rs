use std::sync::Arc;

use crate::blocklist::Blocklist;
use crate::dns::cache::Cache;
use crate::dns::handler::handle_query;
use crate::dns::upstream::UpstreamMultiplexer;
use crate::error::Result;
use crate::metrics::Metrics;
use tokio::net::UdpSocket;
use tracing::error;

pub async fn run(
    listen_addr: &str,
    upstream_addr: &str,
    blocklist: Arc<Blocklist>,
    cache: Cache,
    metrics: Arc<Metrics>,
) -> Result<()> {
    let socket = Arc::new(UdpSocket::bind(listen_addr).await?);
    let shared_upstream = Arc::new(upstream_addr.to_string());

    let upstream_socket = Arc::new(UdpSocket::bind("0.0.0.0:0").await?);
    let multiplexer = UpstreamMultiplexer::new(upstream_socket);

    let mut buf = [0u8; 512];

    loop {
        let (len, addr) = match socket.recv_from(&mut buf).await {
            Ok(result) => result,
            Err(e) => {
                error!(error = %e, "Failed to receive packet");
                continue;
            }
        };

        let payload = buf[..len].to_vec();
        let task_socket = socket.clone();
        let task_blocklist = blocklist.clone();
        let task_upstream = shared_upstream.clone();
        let task_cache = cache.clone();
        let task_metrics = metrics.clone();
        let task_multiplexer = multiplexer.clone();

        tokio::spawn(async move {
            match handle_query(
                payload,
                &task_blocklist,
                &task_upstream,
                &task_cache,
                &task_metrics,
                &task_multiplexer,
            )
            .await
            {
                Ok(response_bytes) => {
                    if let Err(e) = task_socket.send_to(&response_bytes, addr).await {
                        error!(client_ip = %addr, error = %e, "Failed to send response");
                    }
                }
                Err(e) => error!(client_ip = %addr, error = %e, "Query failed"),
            }
        });
    }
}
