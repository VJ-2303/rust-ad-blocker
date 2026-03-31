use std::{
    num::NonZeroUsize,
    sync::Arc,
    time::{Duration, Instant},
};

use bytes::Bytes;
use lru::LruCache;
use parking_lot::Mutex;

const MAX_CACHE_ENTRIES: usize = 5000;

#[derive(Debug, Clone)]
pub struct Cache {
    store: Arc<Mutex<LruCache<Vec<u8>, CacheEntry>>>,
}

#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub response_bytes: Bytes,
    pub expires_at: Instant,
}

impl Cache {
    pub fn new() -> Self {
        Self {
            store: Arc::new(Mutex::new(LruCache::new(
                NonZeroUsize::new(MAX_CACHE_ENTRIES).unwrap(),
            ))),
        }
    }
    pub fn get(&self, domain: &[u8], original_id: &[u8]) -> Option<Bytes> {
        let mut cache = self.store.lock();
        if let Some(entry) = cache.get(domain) {
            if Instant::now() > entry.expires_at {
                cache.pop(domain);
                return None;
            }
            let cached = entry.response_bytes.clone();

            let mut buf = bytes::BytesMut::with_capacity(cached.len());
            buf.extend_from_slice(&[original_id[0], original_id[1]]);
            buf.extend_from_slice(&cached[2..]);
            return Some(buf.freeze());
        }
        None
    }
    pub fn put(&self, domain: Vec<u8>, response_bytes: Bytes, ttl: u32) {
        let mut cache = self.store.lock();

        cache.put(
            domain,
            CacheEntry {
                response_bytes,
                expires_at: Instant::now() + Duration::from_secs(ttl as u64),
            },
        );
    }
    pub fn clean_expired(&self) {
        let now = Instant::now();

        let mut cache = self.store.lock();

        let expired_key: Vec<Vec<u8>> = cache
            .iter()
            .filter(|(_, entry)| entry.expires_at <= now)
            .map(|(key, _)| key.clone())
            .take(200)
            .collect();

        for key in expired_key {
            cache.pop(&key);
        }
    }
}
