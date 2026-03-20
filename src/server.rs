use crate::blocklist::Blocklist;
use crate::dns::handler;
use crate::error::Result;
use tokio::net::UdpSocket;

pub async fn run(listen_addr: &str, upstream_addr: &str, blocklist: Blocklist) -> Result<()> {
    let socket = UdpSocket::bind(listen_addr).await?;
    println!("UDP Server listening on {}", listen_addr);

    let mut buf = [0u8; 512];

    loop {
        let (len, addr) = match socket.recv_from(&mut buf).await {
            Ok(result) => result,
            Err(e) => {
                eprintln!("Failed to receive packet: {}", e);
                continue;
            }
        };

        let packet_bytes = &buf[..len];

        match handler::handle_query(packet_bytes, &blocklist, upstream_addr).await {
            Ok(response_bytes) => {
                if let Err(e) = socket.send_to(&response_bytes, addr).await {
                    eprintln!("Failed to send response to {}: {}", addr, e);
                }
            }
            Err(e) => {
                eprintln!("Failed to handle query from {}: {}", addr, e);
            }
        }
    }
}
