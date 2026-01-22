pub mod config;
pub mod runner;
pub mod ssh_args;
pub mod supervisor;

use std::io;

pub use config::{Config, ForwardingRule};

pub async fn run(config_path: &str) -> io::Result<()> {
    let config = config::load_config(config_path)?;
    supervisor::run(config).await
}

