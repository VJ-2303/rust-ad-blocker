use std::{collections::HashMap, sync::Arc};

use tokio::sync::RwLock;

#[derive(Debug, Clone)]
pub struct Cache {
    store: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl Cache {
    pub fn new() -> Self {
        Self {
            store: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    pub async fn get(&self, domain: &str, original_id: &[u8]) -> Option<Vec<u8>> {
        let store = self.store.read().await;

        if let Some(cached_response) = store.get(domain) {
            let mut response = cached_response.clone();

            response[0] = original_id[0];
            response[1] = original_id[1];

            return Some(response);
        }
        None
    }
    pub async fn put(&self, domain: String, response_bytes: Vec<u8>) {
        let mut store = self.store.write().await;

        store.insert(domain, response_bytes);
    }
}
