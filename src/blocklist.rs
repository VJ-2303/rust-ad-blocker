use std::{
    collections::HashSet,
    fs::File,
    io::{BufRead, BufReader},
};

pub struct Blocklist {
    domains: HashSet<Vec<u8>>,
}

impl Blocklist {
    pub fn new() -> Self {
        Self {
            domains: HashSet::new(),
        }
    }
    pub fn load(path: &str) -> std::io::Result<Self> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);

        let mut blocklist = Self::new();

        for line in reader.lines() {
            let line = line?;
            let line = line.trim();

            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            blocklist.domains.insert(encode_domain(line));
        }
        Ok(blocklist)
    }
    pub fn is_blocked(&self, domain_bytes: &[u8]) -> bool {
        self.domains.contains(domain_bytes)
    }

    pub fn len(&self) -> usize {
        self.domains.len()
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
