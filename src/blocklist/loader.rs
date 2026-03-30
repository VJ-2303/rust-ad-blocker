use std::u8;

use ahash::AHashSet;

use crate::{blocklist::encode_domain, error::AppError};

pub async fn fetch_remote_blocklist(url: &str) -> Result<AHashSet<Vec<u8>>, AppError> {
    let text = reqwest::get(url).await?.text().await?;

    let mut blocklist: AHashSet<Vec<u8>> = AHashSet::new();

    for line in text.lines() {
        let line = line.trim();

        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            let domain = parts[1];

            if domain == "localhost" || domain == "local" || domain == "broadcasthost" {
                continue;
            }
            blocklist.insert(encode_domain(&domain.to_lowercase()));
        }
    }
    Ok(blocklist)
}
