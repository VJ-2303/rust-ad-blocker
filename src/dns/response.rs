use bytes::{Bytes, BytesMut};

pub fn build_nxdomain_raw(query: &[u8]) -> Bytes {
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
pub fn extract_min_ttl(packet: &[u8]) -> u32 {
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
