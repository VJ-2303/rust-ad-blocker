use std::{
    collections::HashSet,
    fs::File,
    io::{BufRead, BufReader},
};

use tokio::sync::RwLock;

pub struct Blocklist {
    domains: RwLock<HashSet<Vec<u8>>>,
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
            domains: RwLock::new(domains),
        })
    }
    pub async fn is_blocked(&self, domain_bytes: &[u8]) -> bool {
        let guard = self.domains.read().await;
        guard.contains(domain_bytes)
    }

    pub async fn len(&self) -> usize {
        self.domains.read().await.len()
    }
    pub async fn add_domain(&self, domain: &str) {
        let encoded_domain = encode_domain(domain);
        let mut guard = self.domains.write().await;
        guard.insert(encoded_domain);
    }
}

fn encode_domain(domain: &str) -> Vec<u8> {
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
