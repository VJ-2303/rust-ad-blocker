use std::{
    collections::HashMap,
    io::Error,
    sync::{
        Arc,
        atomic::{AtomicU16, Ordering},
    },
};

use bytes::{Bytes, BytesMut};
use std::sync::Mutex;
use tokio::{net::UdpSocket, sync::oneshot};
use tracing::error;

use crate::error::{AppError, Result};

type PendingMap = Arc<Mutex<HashMap<u16, oneshot::Sender<BytesMut>>>>;

#[derive(Clone)]
pub struct UpstreamMultiplexer {
    socket: Arc<UdpSocket>,
    pending: PendingMap,
    next_id: Arc<AtomicU16>,
}

impl UpstreamMultiplexer {
    pub fn new(socket: Arc<UdpSocket>) -> Self {
        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let next_id = Arc::new(AtomicU16::new(0));

        let multiplexer = Self {
            socket: socket.clone(),
            pending: pending.clone(),
            next_id,
        };

        let mut buf = BytesMut::with_capacity(65536);

        tokio::spawn(async move {
            loop {
                buf.reserve(4096);
                match socket.recv_buf_from(&mut buf).await {
                    Ok((len, _addr)) => {
                        let response = buf.split_to(len);
                        let id = u16::from_be_bytes([response[0], response[1]]);

                        let mut map = pending.lock().unwrap();

                        if let Some(sender) = map.remove(&id) {
                            let _ = sender.send(response);
                        }
                    }
                    Err(e) => error!("Mainlman error receiving from upstream: {}", e),
                }
            }
        });
        multiplexer
    }
    pub async fn forward(&self, mut query_bytes: BytesMut, upstream_addr: &str) -> Result<Bytes> {
        let original_id_0 = query_bytes[0];
        let original_id_1 = query_bytes[1];

        let internal_id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let id_bytes = internal_id.to_be_bytes();

        query_bytes[0] = id_bytes[0];
        query_bytes[1] = id_bytes[1];

        let (tx, rx) = oneshot::channel();

        {
            let mut map = self.pending.lock().unwrap();
            map.insert(internal_id, tx);
        }

        self.socket.send_to(&query_bytes, upstream_addr).await?;

        match tokio::time::timeout(std::time::Duration::from_secs(2), rx).await {
            Ok(Ok(mut response)) => {
                response[0] = original_id_0;
                response[1] = original_id_1;

                Ok(response.freeze())
            }
            Ok(Err(_)) => Err(AppError::Dns(crate::error::DnsError::NoQueries)),

            Err(_) => {
                let mut map = self.pending.lock().unwrap();
                map.remove(&internal_id);

                Err(AppError::Io(Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Upstream DNS timeout",
                )))
            }
        }
    }
}
