use crate::blocklist::{
    encoding::{decode_domain, encode_domain},
    persistence::{append_to_file, persist_to_file},
};
use ahash::AHashSet;
use parking_lot::RwLock;

pub mod encoding;
pub mod loader;
pub mod persistence;

pub struct Blocklist {
    pub all_domains: RwLock<AHashSet<Vec<u8>>>,
    pub custom_domains: RwLock<AHashSet<Vec<u8>>>,
    pub custom_path: String,
}

impl Blocklist {
    pub fn load(path: &str) -> std::io::Result<Self> {
        let domains = persistence::load_from_file(path)?;
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
        append_to_file(&self.custom_path, &domain).await
    }

    pub async fn remove_custom_domain(&self, domain: &str) -> std::io::Result<bool> {
        let domain = domain.trim().to_lowercase();
        let encoded = encode_domain(&domain);

        let (was_removed, domains_to_persist) = {
            let mut custom_guard = self.custom_domains.write();
            let removed = custom_guard.remove(&encoded);
            let domains_copy = custom_guard.clone();
            (removed, domains_copy)
        };  // Guard dropped here

        if was_removed {
            persist_to_file(&self.custom_path, &domains_to_persist).await?;
            
            let mut all_guard = self.all_domains.write();
            all_guard.remove(&encoded);
            
            Ok(true)
        } else {
            Ok(false)
        }
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
