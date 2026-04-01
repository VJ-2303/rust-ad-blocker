use std::io;

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
    fn config_helper_message_printer() {
        println!("Please provide a config file");
        println!("Config file struture :");
        println!("");
        println!("```toml");
        println!("listen_addr = \"\"");
        println!("upstream_dns = \"\"");
        println!("blocklist_path = \"\"");
        println!("log_level = \"\"");
        println!("admin_addr = \"\"");
        println!("```");
    }

    pub fn load(path: &str) -> Result<Self, crate::error::ConfigError> {
        let config_str = match std::fs::read_to_string(path) {
            Ok(config_str) => config_str,
            Err(e) if (e.kind() == io::ErrorKind::NotFound) => {
                Config::config_helper_message_printer();
                return Err(crate::error::ConfigError::Io(e));
            }
            Err(e) => {
                return Err(crate::error::ConfigError::Io(e));
            }
        };
        let config: Config = toml::from_str(&config_str)?;
        Ok(config)
    }
}
