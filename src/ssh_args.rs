use std::path::PathBuf;

use crate::config::ForwardingRule;

#[derive(Debug, Clone)]
pub struct Invocation {
    pub program: String,
    pub args: Vec<String>,
}

fn parse_host_port(s: &str) -> Result<(String, u16), String> {
    // Supports "host:port" and "[ipv6]:port"
    if let Some(rest) = s.strip_prefix('[') {
        let (host, rest) = rest
            .split_once(']')
            .ok_or_else(|| format!("Invalid address '{}': missing ']'", s))?;
        let port = rest
            .strip_prefix(':')
            .ok_or_else(|| format!("Invalid address '{}': missing port", s))?
            .parse::<u16>()
            .map_err(|e| format!("Invalid port in '{}': {}", s, e))?;
        return Ok((host.to_string(), port));
    }

    let mut it = s.rsplitn(2, ':');
    let port_str = it
        .next()
        .ok_or_else(|| format!("Invalid address '{}': missing port", s))?;
    let host = it
        .next()
        .ok_or_else(|| format!("Invalid address '{}': missing host", s))?;
    let port = port_str
        .parse::<u16>()
        .map_err(|e| format!("Invalid port in '{}': {}", s, e))?;
    Ok((host.to_string(), port))
}

fn expand_tilde_path(p: &str) -> PathBuf {
    PathBuf::from(shellexpand::tilde(p).to_string())
}

pub fn build_invocation(rule: &ForwardingRule) -> Result<Invocation, String> {
    let (dst_host, dst_port) = parse_host_port(&rule.remote_address)?;

    let use_password = rule.ssh_password.is_some();
    let mut ssh_args: Vec<String> = Vec::new();

    // Keep running; port-forward only
    ssh_args.push("-N".to_string());
    // Exit immediately if forwarding setup fails (so the supervisor can restart)
    ssh_args.push("-o".to_string());
    ssh_args.push("ExitOnForwardFailure=yes".to_string());
    // KeepAlive: detect disconnects and exit promptly
    ssh_args.push("-o".to_string());
    ssh_args.push("ServerAliveInterval=30".to_string());
    ssh_args.push("-o".to_string());
    ssh_args.push("ServerAliveCountMax=3".to_string());
    ssh_args.push("-o".to_string());
    ssh_args.push("TCPKeepAlive=yes".to_string());
    // Unified PTY mode: PTY can handle all interactive prompts (password, passphrase, host key, etc.)
    // We don't use BatchMode since PTY handles all interactions.
    // For password mode, limit password prompts to avoid infinite loops.
    if use_password {
        ssh_args.push("-o".to_string());
        ssh_args.push("NumberOfPasswordPrompts=1".to_string());
    }
    // Connection timeout
    ssh_args.push("-o".to_string());
    ssh_args.push("ConnectTimeout=10".to_string());

    // Add -g option to allow remote hosts to connect to local forwarded ports
    // Only needed when binding to non-localhost addresses (e.g., 0.0.0.0)
    if rule.local_bind != "127.0.0.1" && rule.local_bind != "localhost" {
        ssh_args.push("-g".to_string());
    }

    let forward_spec = format!(
        "{}:{}:{}:{}",
        rule.local_bind, rule.local_port, dst_host, dst_port
    );
    ssh_args.push("-L".to_string());
    ssh_args.push(forward_spec);

    ssh_args.push("-p".to_string());
    ssh_args.push(rule.ssh_port.to_string());

    if let Some(key_path) = &rule.ssh_key_path {
        let kp = expand_tilde_path(key_path);
        if !kp.exists() {
            return Err(format!("SSH key not found: {}", kp.display()));
        }
        ssh_args.push("-i".to_string());
        ssh_args.push(kp.to_string_lossy().to_string());
    }

    // Pass through extra ssh args (e.g. -J / ProxyCommand / StrictHostKeyChecking)
    ssh_args.extend(rule.ssh_extra_args.iter().cloned());

    // Target
    ssh_args.push(format!("{}@{}", rule.ssh_user, rule.ssh_host));

    Ok(Invocation {
        program: "ssh".to_string(),
        args: ssh_args,
    })
}

