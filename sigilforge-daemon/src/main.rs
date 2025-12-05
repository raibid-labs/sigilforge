//! Sigilforge Daemon
//!
//! Background service that manages credentials and exposes a local API
//! for Sigilforge clients.
//!
//! # Running
//!
//! ```bash
//! cargo run -p sigilforge-daemon
//! # or after install:
//! sigilforged
//! ```

use anyhow::Result;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

mod config;

#[tokio::main]
async fn main() -> Result<()> {
    init_logging();

    info!("Starting Sigilforge daemon...");

    let config = config::load_config()?;
    info!("Loaded configuration from {:?}", config.config_path);

    run_daemon(config).await
}

fn init_logging() {
    FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_target(false)
        .init();
}

async fn run_daemon(config: config::DaemonConfig) -> Result<()> {
    info!("Daemon running on {:?}", config.socket_path);

    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
        info!("Daemon heartbeat");
    }
}
