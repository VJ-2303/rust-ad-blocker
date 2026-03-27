use std::{collections::HashSet, u8};

use crate::{blocklist::encode_domain, error::AppError};

pub async fn fetch_remote_blocklist(url: &str) -> Result<HashSet<Vec<u8>>, AppError> {
    let text = reqwest::get(url).await?.text().await?;

    let mut blocklist: HashSet<Vec<u8>> = HashSet::new();

    for line in text.lines() {
        let line = line.trim();

        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        blocklist.insert(encode_domain(line));
    }
    Ok(blocklist)
}
