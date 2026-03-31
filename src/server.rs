use std::net::SocketAddr;
use std::sync::Arc;

use crate::blocklist::Blocklist;
use crate::dns::cache::Cache;
use crate::dns::handler::handle_query;
use crate::dns::upstream::UpstreamMultiplexer;
use crate::error::Result;
use crate::metrics::Metrics;
use bytes::BytesMut;
use tracing::error;

#[derive(Clone)]
pub struct ServerState {
    pub socket: Arc<tokio::net::UdpSocket>,
    pub blocklist: Arc<Blocklist>,
    pub cache: Cache,
    pub metrics: Arc<Metrics>,
    pub multiplexer: UpstreamMultiplexer,
    pub upstream_addr: SocketAddr,
}

pub async fn run(state: ServerState) -> Result<()> {
    let mut buf = BytesMut::with_capacity(65536);

    loop {
        buf.reserve(4096);
        let (len, addr) = match state.socket.recv_buf_from(&mut buf).await {
            Ok(result) => result,
            Err(e) => {
                error!(error = %e, "Socket receive error - skipping packet");
                continue;
            }
        };

        let payload: BytesMut = buf.split_to(len);

        let task_state = state.clone();

        tokio::spawn(async move {
            match handle_query(payload, &task_state).await {
                Ok(response_bytes) => {
                    if let Err(e) = task_state.socket.send_to(&response_bytes, addr).await {
                        error!(client_ip = %addr, error = %e, "Failed to send response");
                    }
                }
                Err(e) => error!(client_ip = %addr, error = %e, "Query failed"),
            }
        });
    }
}
