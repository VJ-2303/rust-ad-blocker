use crate::{
    dns::{cache::Cache, upstream::UpstreamMultiplexer},
    error::{AppError, DnsError, Result},
    metrics::Metrics,
};
use tracing::info;

use crate::{blocklist::Blocklist, dns::packet::DnsPacket};

pub async fn handle_query(
    packet_bytes: Vec<u8>,
    blocklist: &Blocklist,
    upstream_addr: &str,
    cache: &Cache,
    metrics: &Metrics,
    multiplexer: &UpstreamMultiplexer,
) -> Result<Vec<u8>> {
    metrics
        .total_queries
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut i = 12;
    while packet_bytes[i] != 0 {
        i += 1
    }
    let domain_bytes = &packet_bytes[12..=i];

    if blocklist.is_blocked(domain_bytes).await {
        metrics
            .blocked_queries
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dns_packet = DnsPacket::parse(&packet_bytes)?;

        let raw_domain = match dns_packet.get_domain() {
            Some(domain) => domain,
            None => return Err(AppError::Dns(DnsError::NoQueries)),
        };

        info!(domain = %raw_domain, status = "BLOCKED", "Query denied by blocklist");

        let response = dns_packet.make_nxdomain();
        let bytes = response.serialize()?;
        Ok(bytes)
    } else {
        if let Some(cached_bytes) = cache.get(domain_bytes, &packet_bytes[0..2]).await {
            return Ok(cached_bytes);
        }

        let upstream_response: Vec<u8> = multiplexer
            .forward(packet_bytes.clone(), upstream_addr)
            .await?;

        let parsed = DnsPacket::parse(&upstream_response)?;

        cache
            .put(
                domain_bytes.to_vec(),
                upstream_response.clone(),
                parsed.get_ttl(),
            )
            .await;

        Ok(upstream_response)
    }
}
