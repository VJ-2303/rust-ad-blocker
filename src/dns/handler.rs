use std::time::Instant;

use crate::dns::response::{build_nxdomain_raw, extract_min_ttl};
use bytes::{Bytes, BytesMut};

use crate::{
    error::{DnsError, Result},
    server::ServerState,
};

pub async fn handle_query(packet_bytes: BytesMut, state: &ServerState) -> Result<Bytes> {
    state
        .metrics
        .total_queries
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let mut domain_buf = [0u8; 255];

    let (domain_bytes, _) = extract_domain_bytes(&packet_bytes, &mut domain_buf)?;

    if state.blocklist.is_blocked(domain_bytes) {
        state
            .metrics
            .blocked_queries
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        Ok(build_nxdomain_raw(&packet_bytes))
    } else {
        if let Some(cached_bytes) = state.cache.get(domain_bytes, &packet_bytes[0..2]) {
            state
                .metrics
                .cache_hits
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return Ok(cached_bytes);
        }
        let domain_owned = domain_bytes.to_vec();
        state
            .metrics
            .cache_misses
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let start_time = Instant::now();
        let upstream_result = state
            .multiplexer
            .forward(packet_bytes, state.upstream_addr)
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

                let ttl = extract_min_ttl(&upstream_response);

                state
                    .cache
                    .put(domain_owned, upstream_response.clone(), ttl);

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

fn extract_domain_bytes<'a>(
    packet_bytes: &'a [u8],
    buf: &'a mut [u8; 255],
) -> std::result::Result<(&'a [u8], usize), DnsError> {
    if packet_bytes.len() < 13 {
        return Err(DnsError::MalformedPacket("packet too short.".into()).into());
    }

    let mut i = 12;

    while i < packet_bytes.len() && packet_bytes[i] != 0 {
        i += 1;
    }
    let domain_len = i - 12 + 1;
    if domain_len > 253 || 12 + domain_len > packet_bytes.len() {
        return Err(DnsError::MalformedPacket("domain name too long.".into()).into());
    }

    buf[..domain_len].copy_from_slice(&packet_bytes[12..12 + domain_len]);

    for byte in buf[..domain_len].iter_mut() {
        byte.make_ascii_lowercase();
    }

    Ok((&buf[..domain_len], domain_len))
}
