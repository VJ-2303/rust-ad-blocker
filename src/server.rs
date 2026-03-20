use crate::blocklist::Blocklist;
use crate::dns::cache::Cache;
use crate::dns::handler;
use crate::error::Result;
use tokio::net::UdpSocket;
use tracing::error;

pub async fn run(
    listen_addr: &str,
    upstream_addr: &str,
    blocklist: Blocklist,
    cache: Cache,
) -> Result<()> {
    let socket = UdpSocket::bind(listen_addr).await?;

    let mut buf = [0u8; 512];

    loop {
        let (len, addr) = match socket.recv_from(&mut buf).await {
            Ok(result) => result,
            Err(e) => {
                error!(error = %e, "Failed to receive packet");
                continue;
            }
        };

        let packet_bytes = &buf[..len];

        match handler::handle_query(packet_bytes, &blocklist, upstream_addr, &cache).await {
            Ok(response_bytes) => {
                if let Err(e) = socket.send_to(&response_bytes, addr).await {
                    error!(
                        client_ip = %addr,
                        error = %e,
                        "Failed to send response back to client"
                    );
                }
            }
            Err(e) => {
                error!(
                    client_ip = %addr,
                    error = %e,
                    "Failed to process the DNS query"
                );
            }
        }
    }
}
