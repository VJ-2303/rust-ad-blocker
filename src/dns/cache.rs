use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use dashmap::DashMap;

#[derive(Debug, Clone)]
pub struct Cache {
    store: Arc<DashMap<Vec<u8>, CacheEntry>>,
}

#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub response_bytes: Vec<u8>,
    pub expires_at: Instant,
}

impl Cache {
    pub fn new() -> Self {
        Self {
            store: Arc::new(DashMap::new()),
        }
    }
    pub async fn get(&self, domain: &[u8], original_id: &[u8]) -> Option<Vec<u8>> {
        if let Some(cached_entry) = self.store.get(domain) {
            if Instant::now() > cached_entry.expires_at {
                return None;
            }
            let mut response = cached_entry.response_bytes.clone();
            response[0] = original_id[0];
            response[1] = original_id[1];

            return Some(response);
        }
        None
    }
    pub async fn put(&self, domain: Vec<u8>, response_bytes: Vec<u8>, ttl: u32) {
        self.store.insert(
            domain,
            CacheEntry {
                response_bytes,
                expires_at: Instant::now() + Duration::from_secs(ttl as u64),
            },
        );
    }
    pub async fn clean_expired(&self) {
        let now = Instant::now();
        self.store.retain(|_domain, entry| entry.expires_at > now);
    }
}
