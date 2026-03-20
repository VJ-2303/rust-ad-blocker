use crate::{
    dns::cache::Cache,
    error::{AppError, DnsError, Result},
};
use tracing::info;

use crate::{
    blocklist::Blocklist,
    dns::{packet::DnsPacket, upstream},
};

pub async fn handle_query(
    packet_bytes: &[u8],
    blocklist: &Blocklist,
    upstream_addr: &str,
    cache: &Cache,
) -> Result<Vec<u8>> {
    let dns_packet = DnsPacket::parse(packet_bytes)?;

    let raw_domain = match dns_packet.get_domain() {
        Some(domain) => domain,
        None => return Err(AppError::Dns(DnsError::NoQueries)),
    };

    if blocklist.is_blocked(&raw_domain) {
        info!(domain = %raw_domain, status = "BLOCKED", "Query denied by blocklist");

        let response = dns_packet.make_nxdomain();
        let bytes = response.serialize()?;
        Ok(bytes)
    } else {
        info!(domain = %raw_domain, status = "ALLOWED", "Query allowed by blocklist");

        if let Some(cached_bytes) = cache.get(&raw_domain, &packet_bytes[0..2]).await {
            return Ok(cached_bytes);
        }

        let upstream_response = upstream::forward(packet_bytes, upstream_addr).await?;

        cache.put(raw_domain, upstream_response.clone()).await;

        Ok(upstream_response)
    }
}
