mod admin;
mod blocklist;
mod config;
mod dns;
mod error;
mod metrics;
mod server;

use std::error::Error;

use crate::blocklist::Blocklist;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let config = config::Config::load("config.toml")?;

    println!("Loading blocklist....");

    let blocklist = Blocklist::load(&config.blocklist_path)?;

    println!(
        "Successfully loaded {} domains into the blocklist!",
        blocklist.len()
    );

    server::run(&config.listen_addr, &config.upstream_dns, blocklist).await?;
    Ok(())
}
