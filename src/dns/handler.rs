use std::time::Instant;

use bytes::{Bytes, BytesMut};

use crate::{error::Result, server::ServerState};
// use tracing::info;

use crate::dns::packet::DnsPacket;

pub async fn handle_query(packet_bytes: BytesMut, state: &ServerState) -> Result<Bytes> {
    state
        .metrics
        .total_queries
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut i = 12;
    while packet_bytes[i] != 0 {
        i += 1
    }
    let domain_bytes = packet_bytes[12..=i].to_vec();

    if state.blocklist.is_blocked(&domain_bytes).await {
        state
            .metrics
            .blocked_queries
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dns_packet = DnsPacket::parse(&packet_bytes)?;

        // let raw_domain = match dns_packet.get_domain() {
        //     Some(domain) => domain,
        //     None => return Err(AppError::Dns(DnsError::NoQueries)),
        // };

        // info!(domain = %raw_domain, status = "BLOCKED", "Query denied by blocklist");

        let response = dns_packet.make_nxdomain();
        let bytes = response.serialize()?;
        Ok(bytes::Bytes::from(bytes))
    } else {
        if let Some(cached_bytes) = state.cache.get(&domain_bytes, &packet_bytes[0..2]).await {
            state
                .metrics
                .cache_hits
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return Ok(bytes::Bytes::from(cached_bytes));
        }
        state
            .metrics
            .cache_misses
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let start_time = Instant::now();
        let upstream_result = state
            .multiplexer
            .forward(packet_bytes, &state.upstream_addr)
            .await;
        let elapsed_ms = start_time.elapsed().as_millis() as u64;

        match upstream_result {
            Ok(upstream_response) => {
                state
                    .metrics
                    .upstream_latency_ms
                    .fetch_add(elapsed_ms, std::sync::atomic::Ordering::Relaxed);
                state
                    .metrics
                    .upstream_requests
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                let parsed = DnsPacket::parse(&upstream_response)?;

                state
                    .cache
                    .put(domain_bytes, upstream_response.to_vec(), parsed.get_ttl())
                    .await;

                Ok(upstream_response)
            }
            Err(e) => {
                state
                    .metrics
                    .upstream_errors
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                Err(e)
            }
        }
    }
}
