use clap::Parser;
use std::path::PathBuf;
use time::macros::format_description;
use time::UtcOffset;
use tracing_subscriber::fmt::time::OffsetTime;

#[derive(Parser)]
#[command(name = "stm", version, about = "Manage SSH port forwarding from a TOML config")]
struct Cli {
    /// Path to the TOML configuration file
    #[arg(short, long, default_value = "config.toml", value_name = "PATH")]
    config: PathBuf,
}

fn init_tracing() -> std::io::Result<()> {
    let local_offset = UtcOffset::current_local_offset().unwrap_or(UtcOffset::UTC);
    let timer = OffsetTime::new(
        local_offset,
        format_description!("[year]-[month]-[day] [hour]:[minute]:[second]"),
    );

    tracing_subscriber::fmt()
        .with_timer(timer)
        .with_target(false)
        .with_ansi(false)
        .try_init()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, format!("init tracing failed: {e}")))
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    init_tracing()?;

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
    stm::run(path).await
}
