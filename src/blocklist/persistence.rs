use super::encoding::{decode_domain, encode_domain};
use ahash::AHashSet;
use std::{
    fs::File,
    io::{BufRead, BufReader},
};
use tokio::io::AsyncWriteExt;

pub fn load_from_file(path: &str) -> std::io::Result<AHashSet<Vec<u8>>> {
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
    Ok(domains)
}

pub async fn persist_to_file(path: &str, domains: &AHashSet<Vec<u8>>) -> std::io::Result<()> {
    let mut sorted: Vec<String> = domains.iter().map(|b| decode_domain(b)).collect();
    sorted.sort();

    let mut lines = String::from("# Custom blocked domains\n");
    for domain in sorted {
        lines.push_str(&domain);
        lines.push('\n');
    }

    tokio::fs::write(path, lines).await?;
    Ok(())
}

pub async fn append_to_file(path: &str, domain: &str) -> std::io::Result<()> {
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;

    file.write_all(format!("\n{}", domain).as_bytes()).await?;
    Ok(())
}
