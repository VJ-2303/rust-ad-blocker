mod admin;
mod blocklist;
mod config;
mod dns;
mod error;
mod metrics;
mod server;

use std::error::Error;

use crate::config::Config;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = Config::load("config.toml")?;
    println!("Server will listen on: {}", config.listen_addr);
    Ok(())
}
