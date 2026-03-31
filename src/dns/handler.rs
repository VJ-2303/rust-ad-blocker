use std::time::Instant;

use bytes::{Bytes, BytesMut};

use crate::{error::Result, server::ServerState};
// use tracing::info;

pub async fn handle_query(packet_bytes: BytesMut, state: &ServerState) -> Result<Bytes> {
    state
        .metrics
        .total_queries
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let mut domain_buf = [0u8; 255];
    let mut i = 12;

    while i < packet_bytes.len() && packet_bytes[i] != 0 {
        i += 1;
    }
    let domain_len = i - 12 + 1;
    domain_buf[..domain_len].copy_from_slice(&packet_bytes[12..12 + domain_len]);

    for byte in domain_buf[..domain_len].iter_mut() {
        byte.make_ascii_lowercase();
    }

    let domain_bytes = &domain_buf[..domain_len];

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
            return Ok(cached_bytes);
        }

        let domain_owned = domain_bytes;
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
                    .put(domain_owned.to_vec(), upstream_response.clone(), ttl);

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

/// Extracts the minimum TTL from answer records by reading raw bytes.
/// Falls back to 300 seconds if no answers are present or parsing fails.
fn extract_min_ttl(packet: &[u8]) -> u32 {
    // Safety: DNS packets must be at least 12 bytes (header)
    if packet.len() < 12 {
        return 300;
    }

    // ANCOUNT is at bytes 6-7 (number of answer records)
    let ancount = u16::from_be_bytes([packet[6], packet[7]]) as usize;
    if ancount == 0 {
        return 300;
    }

    // Skip past the header (12 bytes) and the question section
    let mut pos = 12;

    // QDCOUNT is at bytes 4-5
    let qdcount = u16::from_be_bytes([packet[4], packet[5]]) as usize;

    // Skip each question: labels until null byte, then +4 for QTYPE and QCLASS
    for _ in 0..qdcount {
        // Skip labels
        while pos < packet.len() {
            let len = packet[pos] as usize;
            if len == 0 {
                pos += 1; // skip null terminator
                break;
            }
            if len >= 0xC0 {
                // Compression pointer — 2 bytes total
                pos += 2;
                break;
            }
            pos += 1 + len;
        }
        pos += 4; // QTYPE(2) + QCLASS(2)
    }

    // Now we're at the start of the answer section
    // Read each answer record and find the minimum TTL
    let mut min_ttl: u32 = u32::MAX;

    for _ in 0..ancount {
        if pos >= packet.len() {
            break;
        }

        // Skip NAME (could be a pointer or labels)
        let len = packet[pos] as usize;
        if len >= 0xC0 {
            pos += 2; // compression pointer
        } else {
            while pos < packet.len() {
                let len = packet[pos] as usize;
                if len == 0 {
                    pos += 1;
                    break;
                }
                if len >= 0xC0 {
                    pos += 2;
                    break;
                }
                pos += 1 + len;
            }
        }

        // Now at TYPE(2) + CLASS(2) + TTL(4) + RDLENGTH(2)
        if pos + 10 > packet.len() {
            break;
        }

        let ttl = u32::from_be_bytes([
            packet[pos + 4],
            packet[pos + 5],
            packet[pos + 6],
            packet[pos + 7],
        ]);
        min_ttl = min_ttl.min(ttl);

        let rdlength = u16::from_be_bytes([packet[pos + 8], packet[pos + 9]]) as usize;
        pos += 10 + rdlength; // skip past TYPE+CLASS+TTL+RDLENGTH+RDATA
    }

    if min_ttl == u32::MAX { 300 } else { min_ttl }
}
