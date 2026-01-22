#[tokio::main]
async fn main() -> std::io::Result<()> {
    ssh_tunnel_manager::run("config.toml").await
}
