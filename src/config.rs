use serde::Deserialize;
use std::{fs, io};

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "lowercase")]
pub enum TunnelMode {
    #[default]
    Local,  // -L
    Remote, // -R
}

/// A single port-forward mapping.
#[derive(Deserialize, Debug, Clone)]
pub struct Forward {
    /// "local" (-L) or "remote" (-R). Default: "local"
    #[serde(default)]
    pub mode: TunnelMode,
    /// Local bind address and port, e.g. "127.0.0.1:3316" or "0.0.0.0:8080"
    pub local_address: String,
    /// Remote address host:port, e.g. "db.internal:3306"
    pub remote_address: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ForwardingRule {
    #[serde(default)]
    pub forwards: Vec<Forward>,
    pub ssh_host: String,
    #[serde(default = "default_ssh_port")]
    pub ssh_port: u16,
    pub ssh_user: String,
    #[serde(default)]
    pub ssh_key_path: Option<String>,
    #[serde(default)]
    pub ssh_password: Option<String>,
    #[serde(default)]
    pub ssh_extra_args: Vec<String>,
    #[serde(default = "default_server_alive_interval")]
    pub server_alive_interval: u16,
    #[serde(default = "default_server_alive_count_max")]
    pub server_alive_count_max: u8,
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout: u16,
}

fn default_ssh_port() -> u16 {
    22
}

fn default_server_alive_interval() -> u16 {
    60
}

fn default_server_alive_count_max() -> u8 {
    3
}

fn default_connect_timeout() -> u16 {
    10
}

#[derive(Deserialize, Debug)]
pub struct Config {
    pub forwarding: Vec<ForwardingRule>,
}

pub fn load_config(config_path: &str) -> io::Result<Config> {
    let config_str = fs::read_to_string(config_path)?;
    toml::de::from_str(&config_str)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}
