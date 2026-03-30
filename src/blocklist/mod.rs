use ahash::AHashSet;
use parking_lot::RwLock;
use std::{
    fs::File,
    io::{BufRead, BufReader},
};
use tokio::io::AsyncWriteExt;
pub mod loader;

pub struct Blocklist {
    pub all_domains: RwLock<AHashSet<Vec<u8>>>,
    pub custom_domains: RwLock<AHashSet<Vec<u8>>>,
    pub custom_path: String,
}

impl Blocklist {
    pub fn load(path: &str) -> std::io::Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);

        let mut domains = AHashSet::new();

        for line in reader.lines() {
            let line = line?;
            let line = line.trim();

            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            domains.insert(encode_domain(&line.to_lowercase()));
        }
        let initial_all = domains.clone();
        Ok(Self {
            custom_domains: RwLock::new(domains),
            all_domains: RwLock::new(initial_all),
            custom_path: path.to_string(),
        })
    }
    pub fn is_blocked(&self, domain_bytes: &[u8]) -> bool {
        self.all_domains.read().contains(domain_bytes)
    }

    pub fn get_custom_domains(&self) -> Vec<String> {
        let guard = self.custom_domains.read();
        guard.iter().map(|bytes| decode_domain(bytes)).collect()
    }

    pub async fn add_custom_domain(&self, domain: &str) -> std::io::Result<()> {
        let domain = domain.trim().to_lowercase();
        let encoded = encode_domain(&domain);

        {
            let mut custom_guard = self.custom_domains.write();
            if !custom_guard.insert(encoded.clone()) {
                return Ok(());
            }
        }
        {
            let mut all_guard = self.all_domains.write();
            all_guard.insert(encoded);
        }
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.custom_path)
            .await?;

        file.write_all(format!("\n{}", domain).as_bytes()).await?;
        Ok(())
    }

    pub async fn remove_custom_domain(&self, domain: &str) -> std::io::Result<bool> {
        let domain = domain.trim().to_lowercase();
        let encoded = encode_domain(&domain);

        let was_removed = {
            let mut custom_guard = self.custom_domains.write();
            custom_guard.remove(&encoded)
        };

        if !was_removed {
            return Ok(false);
        }

        {
            let mut all_guard = self.all_domains.write();
            all_guard.remove(&encoded);
        }

        self.persist_custom_domains().await?;
        Ok(true)
    }

    async fn persist_custom_domains(&self) -> std::io::Result<()> {
        let content = {
            let guard = self.custom_domains.read();
            let mut sorted: Vec<String> = guard.iter().map(|b| decode_domain(b)).collect();
            sorted.sort();

            let mut lines = String::from("# Custom blocked domains\n");
            for domain in sorted {
                lines.push_str(&domain);
                lines.push('\n');
            }
            lines
        };
        tokio::fs::write(&self.custom_path, content).await
    }

    pub fn len(&self) -> usize {
        self.all_domains.read().len()
    }
    pub fn update_list(&self, remote: AHashSet<Vec<u8>>) {
        let custom_snapshot = {
            let guard = self.custom_domains.read();
            guard.clone()
        };

        let mut all = remote;
        all.extend(custom_snapshot);

        *self.all_domains.write() = all;
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

pub(crate) fn decode_domain(bytes: &[u8]) -> String {
    let mut domain = String::new();

    let mut i = 0;

    while i < bytes.len() {
        let len = bytes[i] as usize;
        if len == 0 {
            break;
        }
        i += 1;
        if i + len > bytes.len() {
            break;
        }
        if !domain.is_empty() {
            domain.push('.');
        }
        if let Ok(label) = std::str::from_utf8(&bytes[i..i + len]) {
            domain.push_str(label);
        }
        i += len
    }
    domain
}
