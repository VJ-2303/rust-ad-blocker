use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    pub listen_addr: String,
    pub upstream_dns: String,
    pub blocklist_path: String,
    #[serde(default = "Config::default_log_level")]
    pub log_level: String,
    #[serde(default = "Config::default_admin_addr")]
    pub admin_addr: String,
}

impl Config {
    fn default_admin_addr() -> String {
        "0.0.0.0:8080".to_string()
    }
    fn default_log_level() -> String {
        "error".to_string()
    }

    pub fn load(path: &str) -> Result<Self, crate::error::ConfigError> {
        let config_str = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&config_str)?;
        Ok(config)
    }
}
