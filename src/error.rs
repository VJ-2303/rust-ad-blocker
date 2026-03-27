use thiserror::Error;

// Every function just return `crate::error::Result<T>`
pub type Result<T> = std::result::Result<T, AppError>;

// This wraps all the specific department errors.
#[derive(Debug, Error)]
pub enum AppError {
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("DNS processing error: {0}")]
    Dns(#[from] DnsError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
}

// 3. THE DEPARTMENT ERRORS
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Failed to read config file: {0}")]
    Io(#[from] std::io::Error),

    #[error("Failed to parse TOML: {0}")]
    Parse(#[from] toml::de::Error),
}

#[derive(Debug, Error)]
pub enum DnsError {
    #[error("Failed to parse DNS packet: {0}")]
    Parse(#[from] hickory_proto::error::ProtoError),

    // Look! We replaced the "Stringly-Typed" error with a real, strongly-typed variant!
    #[error("DNS packet contained no queries")]
    NoQueries,
}
