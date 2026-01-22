use serde::Deserialize;
use std::{fs, io};

#[derive(Deserialize, Debug, Clone)]
pub struct ForwardingRule {
    pub local_port: u16,
    #[serde(default = "default_local_bind")]
    pub local_bind: String,
    pub remote_address: String,
    pub ssh_host: String,
    #[serde(default = "default_ssh_port")]
    pub ssh_port: u16,
    pub ssh_user: String,
    #[serde(default)]
    pub ssh_key_path: Option<String>,
    #[serde(default)]
    pub ssh_password: Option<String>,
    // Extra arguments passed through to ssh (optional)
    #[serde(default)]
    pub ssh_extra_args: Vec<String>,
}

fn default_ssh_port() -> u16 {
    22
}

fn default_local_bind() -> String {
    // Keep consistent with legacy behavior: listen on 0.0.0.0
    "0.0.0.0".to_string()
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

