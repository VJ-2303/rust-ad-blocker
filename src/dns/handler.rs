use std::error::Error;

use crate::{
    blocklist::Blocklist,
    dns::{packet::DnsPacket, upstream},
};

pub async fn handle_query(
    packet_bytes: &[u8],
    blocklist: &Blocklist,
    upstream_addr: &str,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let dns_packet = DnsPacket::parse(packet_bytes)?;

    let raw_domain = match dns_packet.get_domain() {
        Some(domain) => domain,
        None => return Err("packet had no queries".into()),
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
