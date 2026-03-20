use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub listen_addr: String,
    pub upstream_dns: String,
    pub blocklist_path: String,
    pub log_level: String,
}

impl Config {
    pub fn load(path: &str) -> Result<Self, crate::error::ConfigError> {
        let config_str = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&config_str)?;
        Ok(config)
    }
}
