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
    while i < packet_bytes.len() && packet_bytes[i] != 0 {
        i += 1
    }
    let mut domain_bytes = packet_bytes[12..=i].to_vec();

    for byte in domain_bytes.iter_mut() {
        byte.make_ascii_lowercase();
    }

    let domain_bytes = domain_bytes;

    if state.blocklist.is_blocked(&domain_bytes) {
        state
            .metrics
            .blocked_queries
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        Ok(build_nxdomain_raw(&packet_bytes))
    } else {
        if let Some(cached_bytes) = state.cache.get(&domain_bytes, &packet_bytes[0..2]) {
            state
                .metrics
                .cache_hits
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return Ok(bytes::Bytes::from(cached_bytes));
        }

        let domain_owned = domain_bytes;
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
                    .put(domain_owned, upstream_response.to_vec(), parsed.get_ttl());

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

fn build_nxdomain_raw(query: &[u8]) -> Bytes {
    // Find end of question section: skip labels until null terminator, then +4 for QTYPE+QCLASS
    let mut i = 12;
    while i < query.len() && query[i] != 0 {
        i += query[i] as usize + 1;
    }
    let question_end = i + 1 + 4; // null byte + QTYPE(2) + QCLASS(2)

    let response_len = question_end; // header(12) + question section
    let mut buf = BytesMut::with_capacity(response_len);

    // Transaction ID — same as query
    buf.extend_from_slice(&query[0..2]);

    // Flags: QR=1, Opcode=0, AA=0, TC=0, RD=1 | RA=1, Z=0, RCODE=3 (NXDOMAIN)
    buf.extend_from_slice(&[0x81, 0x83]);

    // QDCOUNT — keep from query (usually 0x00 0x01)
    buf.extend_from_slice(&query[4..6]);

    // ANCOUNT, NSCOUNT, ARCOUNT — all zero
    buf.extend_from_slice(&[0, 0, 0, 0, 0, 0]);

    // Question section — verbatim copy from query
    buf.extend_from_slice(&query[12..question_end]);

    buf.freeze()
}
