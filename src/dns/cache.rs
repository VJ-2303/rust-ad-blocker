use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Ok;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct Cache {
    store: Arc<RwLock<HashMap<Vec<u8>, CacheEntry>>>,
}

#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub response_bytes: Vec<u8>,
    pub expires_at: Instant,
}

impl Cache {
    pub fn new() -> Self {
        Self {
            store: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    pub async fn get(&self, domain: &[u8], original_id: &[u8]) -> Option<Vec<u8>> {
        let mut store = self.store.write().await;

        if let Some(cached_entry) = store.get(domain) {
            if Instant::now() > cached_entry.expires_at {
                store.remove(domain);
                return None;
            }
            let mut response = cached_entry.response_bytes.clone();
            response[0] = original_id[0];
            response[1] = original_id[1];

            return Some(response);
        }
        None
    }
    pub async fn put(&self, domain: Vec<u8>, response_bytes: Vec<u8>) {
        let mut store = self.store.write().await;

        let entry = CacheEntry {
            response_bytes,
            expires_at: Instant::now() + Duration::from_secs(300),
        };
        store.insert(domain, entry);
    }
}
