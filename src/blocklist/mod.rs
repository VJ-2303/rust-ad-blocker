use std::{
    collections::HashSet,
    fs::File,
    io::{BufRead, BufReader},
};
pub mod loader;

use tokio::sync::RwLock;

pub struct Blocklist {
    pub remote_domains: RwLock<HashSet<Vec<u8>>>,
    pub custom_domains: RwLock<HashSet<Vec<u8>>>,
}

impl Blocklist {
    pub fn load(path: &str) -> std::io::Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);

        let mut domains = HashSet::new();

        for line in reader.lines() {
            let line = line?;
            let line = line.trim();

            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            domains.insert(encode_domain(line));
        }
        Ok(Self {
            custom_domains: RwLock::new(domains),
            remote_domains: RwLock::new(HashSet::new()),
        })
    }
    pub async fn is_blocked(&self, domain_bytes: &[u8]) -> bool {
        if self.custom_domains.read().await.contains(domain_bytes) {
            return true;
        }
        self.remote_domains.read().await.contains(domain_bytes)
    }

    pub async fn len(&self) -> usize {
        self.custom_domains.read().await.len() + self.remote_domains.read().await.len()
    }
    pub async fn update_list(&self, new_domains: HashSet<Vec<u8>>) {
        let mut guard = self.remote_domains.write().await;
        *guard = new_domains;
    }
}

pub(crate) fn encode_domain(domain: &str) -> Vec<u8> {
    let mut bytes: Vec<u8> = Vec::new();
    for label in domain.split('.') {
        let len = label.len();
        if len > 0 {
            bytes.push(len as u8);
            bytes.extend_from_slice(label.as_bytes());
        }
    }
    bytes.push(0);
    bytes
}
