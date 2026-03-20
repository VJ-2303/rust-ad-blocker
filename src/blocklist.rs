use std::{
    collections::HashSet,
    fs::File,
    io::{BufRead, BufReader},
};

pub struct Blocklist {
    domains: HashSet<String>,
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
            blocklist.domains.insert(line.to_string());
        }
        Ok(blocklist)
    }
    fn contains(&self, domain: &str) -> bool {
        self.domains.contains(domain)
    }
    pub fn is_blocked(&self, raw_query: &str) -> bool {
        let mut domain = raw_query.to_string();
        if domain.ends_with('.') {
            domain.pop();
        }
        self.contains(&domain)
    }

    // A quick helper to get the count since `domains` is private now
    pub fn len(&self) -> usize {
        self.domains.len()
    }
}
