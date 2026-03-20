use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "ssh-tunnel-manager", version, about = "Manage SSH port forwarding from a TOML config")]
struct Cli {
    /// Path to the TOML configuration file
    #[arg(short, long, default_value = "config.toml", value_name = "PATH")]
    config: PathBuf,
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let cli = Cli::parse();
    let path = cli
        .config
        .to_str()
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "config path is not valid UTF-8",
            )
        })?;
    ssh_tunnel_manager::run(path).await
}
