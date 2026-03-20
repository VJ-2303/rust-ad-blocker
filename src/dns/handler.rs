use crate::error::{AppError, DnsError, Result};
use std::error::Error;

use crate::{
    blocklist::Blocklist,
    dns::{packet::DnsPacket, upstream},
};

pub async fn handle_query(
    packet_bytes: &[u8],
    blocklist: &Blocklist,
    upstream_addr: &str,
) -> Result<Vec<u8>> {
    let dns_packet = DnsPacket::parse(packet_bytes)?;

    let raw_domain = match dns_packet.get_domain() {
        Some(domain) => domain,
        None => return Err(AppError::Dns(DnsError::NoQueries)),
    };

    if blocklist.is_blocked(&raw_domain) {
        println!("BLOCKED: {}", raw_domain);

        let response = dns_packet.make_nxdomain();
        let bytes = response.serialize()?;
        Ok(bytes)
    } else {
        println!("ALLOWED: {}", raw_domain);
        let upstream_response = upstream::forward(packet_bytes, upstream_addr).await?;
        Ok(upstream_response)
    }
}
