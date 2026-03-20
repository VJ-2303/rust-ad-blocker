use crate::dns::packet;
use std::error::Error;
use tokio::net::UdpSocket;

pub async fn run(listen_addr: &str) -> Result<(), Box<dyn Error>> {
    let socket = UdpSocket::bind(listen_addr).await?;
    println!("UDP Server listening on {}", listen_addr);

    let mut buf = [0u8; 512];

    loop {
        let result = socket.recv_from(&mut buf).await;

        match result {
            Ok((len, addr)) => {
                println!("Received {} bytes from {}", len, addr);

                let packet_buffer = &buf[..len];

                match packet::parse(packet_buffer) {
                    Ok(dns_packet) => {
                        println!(
                            "Successfully parsed DNS query from {}: {:#?}",
                            addr, dns_packet
                        );
                    }
                    Err(e) => {
                        eprintln!("Failed to parse DNS packet from {}: {}", addr, e);
                    }
                }
            }
            Err(e) => {
                eprintln!("Failed to receive packet: {}", e);
                continue;
            }
        }
    }
}
