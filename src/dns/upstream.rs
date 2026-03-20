use std::io;

use tokio::net::UdpSocket;

pub async fn forward(query_bytes: &[u8], upsteam_addr: &str) -> io::Result<Vec<u8>> {
    let socket = UdpSocket::bind("0.0.0.0:0").await?;

    socket.connect(upsteam_addr).await?;

    socket.send(query_bytes).await?;

    let mut buf = [0u8; 4096];

    let len = socket.recv(&mut buf).await?;

    Ok(buf[..len].to_vec())
}
