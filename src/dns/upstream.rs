use std::{
    io::Error,
    sync::{
        Arc,
        atomic::{AtomicU16, Ordering},
    },
};

use bytes::{Bytes, BytesMut};
use dashmap::DashMap;
use tokio::{net::UdpSocket, sync::oneshot};
use tracing::error;

use crate::error::{AppError, Result};

type PendingMap = Arc<DashMap<u16, oneshot::Sender<BytesMut>>>;

#[derive(Clone)]
pub struct UpstreamMultiplexer {
    sockets: [Arc<UdpSocket>; 2],
    pending: PendingMap,
    next_id: Arc<AtomicU16>,
}

impl UpstreamMultiplexer {
    pub fn new(socket_a: Arc<UdpSocket>, socket_b: Arc<UdpSocket>) -> Self {
        let pending: PendingMap = Arc::new(DashMap::new());
        let next_id = Arc::new(AtomicU16::new(0));

        let multiplexer = Self {
            sockets: [socket_a.clone(), socket_b.clone()],
            pending: pending.clone(),
            next_id,
        };

        for socket in [socket_a, socket_b] {
            let pending = pending.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];

                loop {
                    match socket.recv_from(&mut buf).await {
                        Ok((len, _addr)) => {
                            if len < 12 {
                                continue;
                            }
                            let id = u16::from_be_bytes([buf[0], buf[1]]);
                            if let Some((_, sender)) = pending.remove(&id) {
                                let response = BytesMut::from(&buf[..len]);
                                let _ = sender.send(response);
                            }
                        }
                        Err(e) => error!("error receiving from upstream: {}", e),
                    }
                }
            });
        }

        multiplexer
    }
    pub async fn forward(&self, mut query_bytes: BytesMut, upstream_addr: &str) -> Result<Bytes> {
        let original_id_0 = query_bytes[0];
        let original_id_1 = query_bytes[1];

        let internal_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let id_bytes = internal_id.to_be_bytes();

        let socket = &self.sockets[(internal_id & 1) as usize];

        query_bytes[0] = id_bytes[0];
        query_bytes[1] = id_bytes[1];

        let (tx, mut rx) = oneshot::channel();
        self.pending.insert(internal_id, tx);

        let max_attempts = 2;
        let timeout_per_attempt = std::time::Duration::from_secs(3);

        for attempt in 0..max_attempts {
            if let Err(e) = socket.send_to(&query_bytes, upstream_addr).await {
                self.pending.remove(&internal_id);
                return Err(AppError::Io(e));
            }

            match tokio::time::timeout(timeout_per_attempt, &mut rx).await {
                Ok(Ok(mut response)) => {
                    response[0] = original_id_0;
                    response[1] = original_id_1;
                    return Ok(response.freeze());
                }
                Ok(Err(_)) => {
                    self.pending.remove(&internal_id);
                    return Err(AppError::Dns(crate::error::DnsError::UpstreamChannelClosed));
                }
                Err(_) => {
                    if attempt < max_attempts - 1 {
                        continue;
                    }
                }
            }
        }
        self.pending.remove(&internal_id);
        Err(AppError::Io(Error::new(
            std::io::ErrorKind::TimedOut,
            "Upstream DNS timeout",
        )))
    }
}
