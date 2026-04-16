use serde::Deserialize;
use std::{fs, io};

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "lowercase")]
pub enum TunnelMode {
    #[default]
    Local,  // -L (default, backward compatible)
    Remote, // -R
}

/// A single port-forward mapping within a rule.
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

/// Internal resolved struct used after deserialization normalization.
#[derive(Debug, Clone)]
pub struct ForwardingRule {
    pub forwards: Vec<Forward>,
    pub ssh_host: String,
    pub ssh_port: u16,
    pub ssh_user: String,
    pub ssh_key_path: Option<String>,
    pub ssh_password: Option<String>,
    pub ssh_extra_args: Vec<String>,
}

/// Raw deserialization struct supporting both single-port shorthand and multi-port format.
#[derive(Deserialize, Debug)]
struct ForwardingRuleRaw {
    /// Single-port shorthand: mode at rule level
    #[serde(default)]
    pub mode: TunnelMode,
    /// Single-port shorthand: local_address at rule level
    pub local_address: Option<String>,
    /// Single-port shorthand: remote_address at rule level
    pub remote_address: Option<String>,
    /// Multi-port: nested forwards
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
}

fn default_ssh_port() -> u16 {
    22
}

impl TryFrom<ForwardingRuleRaw> for ForwardingRule {
    type Error = String;

    fn try_from(raw: ForwardingRuleRaw) -> Result<Self, Self::Error> {
        let forwards = if !raw.forwards.is_empty() {
            if raw.local_address.is_some() || raw.remote_address.is_some() {
                return Err(
                    "Cannot specify both forwards[] and top-level local_address/remote_address"
                        .to_string(),
                );
            }
            raw.forwards
        } else {
            match (raw.local_address, raw.remote_address) {
                (Some(la), Some(ra)) => vec![Forward {
                    mode: raw.mode,
                    local_address: la,
                    remote_address: ra,
                }],
                (Some(_), None) => {
                    return Err("remote_address is required when using single-port format".to_string())
                }
                (None, Some(_)) => {
                    return Err("local_address is required when using single-port format".to_string())
                }
                (None, None) => {
                    return Err("Either forwards[] or local_address+remote_address must be specified".to_string())
                }
            }
        };

        Ok(ForwardingRule {
            forwards,
            ssh_host: raw.ssh_host,
            ssh_port: raw.ssh_port,
            ssh_user: raw.ssh_user,
            ssh_key_path: raw.ssh_key_path,
            ssh_password: raw.ssh_password,
            ssh_extra_args: raw.ssh_extra_args,
        })
    }
}

#[derive(Deserialize, Debug)]
struct ConfigRaw {
    forwarding: Vec<ForwardingRuleRaw>,
}

pub struct Config {
    pub forwarding: Vec<ForwardingRule>,
}

pub fn load_config(config_path: &str) -> io::Result<Config> {
    let config_str = fs::read_to_string(config_path)?;
    let raw: ConfigRaw = toml::de::from_str(&config_str)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let forwarding = raw
        .forwarding
        .into_iter()
        .map(|r| {
            ForwardingRule::try_from(r)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        })
        .collect::<io::Result<Vec<_>>>()?;

    Ok(Config { forwarding })
}
