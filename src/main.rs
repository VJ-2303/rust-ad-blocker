mod admin;
mod blocklist;
mod config;
mod dns;
mod error;
mod metrics;
mod server;

use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = config::Config::load("config.toml")?;
    server::run(&config.listen_addr).await?;
    Ok(())
}
