# stm

English | [简体中文](README.zh-CN.md)

A tool to manage **multiple long-running SSH port forwards** from `config.toml`. Each `[[forwarding]]` rule runs a persistent `ssh -N -L/-R ...` process with auto-reconnect, supporting multiple port forwards in a single SSH connection.

### Why this tool?

SSH itself can detect disconnects using keepalive options (`ServerAliveInterval`, `ServerAliveCountMax`, `TCPKeepAlive`), but **SSH cannot automatically restart** when it exits. If you just run `ssh -N -L ...` manually, you'll need to manually restart it every time it disconnects.

The core value of this tool is **auto-restart**: it automatically reconnects when SSH exits due to network issues, and intelligently handles authentication failures (stops retrying to avoid log spam). See the "Features" section below for details.

### Features

- **Multiple rules in parallel**: one SSH tunnel per `[[forwarding]]`
- **Auto-reconnect**: exponential backoff on non-auth failures
- **No retry on auth failure**: if `Permission denied` / `Authentication failed` is detected, that rule stops (prevents log spam)
- **Exit behavior**: exits on `Ctrl-C`, or automatically when all rules have finished

### Requirements

- **System `ssh`** available in `PATH`
  - macOS/Linux: usually preinstalled
  - Windows: install OpenSSH (or ensure `ssh.exe` is available)

### Install

```bash
# From GitHub
cargo install --git https://github.com/miiy/ssh-tunnel-manager.git

# Or from local source
cargo install --path .
```

Then run with your config:

```bash
stm                    # uses config.toml in current directory
stm /path/to/config    # uses specified config file
```

### Configuration (`config.toml`)

Structure:

- `[[forwarding]]`: one forwarding rule (repeatable)
  - **forwards**: array of port-forward mappings (required)
    - **mode**: `"local"` (-L) or `"remote"` (-R) (optional, default `"local"`)
    - **local_address**: local bind address and port, e.g. `"127.0.0.1:3316"` or `"0.0.0.0:8080"` (required)
    - **remote_address**: remote target `host:port` (required; supports `[ipv6]:port`)
  - **ssh_host**: SSH destination (host/IP, or a `Host` alias from `~/.ssh/config`)
  - **ssh_port**: SSH port (optional, default `22`)
  - **ssh_user**: SSH username (required)
  - **ssh_key_path**: private key path (optional; supports `~`; recommended)
  - **ssh_password**: password (optional; PTY will automatically answer password/passphrase prompts)
  - **server_alive_interval**: SSH `ServerAliveInterval` in seconds (optional, default `60`)
  - **server_alive_count_max**: SSH `ServerAliveCountMax` (optional, default `3`)
  - **connect_timeout**: SSH `ConnectTimeout` in seconds (optional, default `10`)
  - **ssh_extra_args**: extra args passed through to `ssh` (optional)

Example:

```toml
[[forwarding]]
ssh_host = "bastion.example.com"
ssh_user = "deploy"
ssh_key_path = "~/.ssh/id_ed25519"
forwards = [
  { local_address = "127.0.0.1:3316", remote_address = "db.internal:3306" },
  { mode = "remote", local_address = "127.0.0.1:9090", remote_address = "0.0.0.0:9090" },
]
```

### Architecture

This tool uses a multi-layered thread nesting architecture to handle blocking PTY operations while maintaining async concurrency:

```
Main Thread (Tokio Runtime)
└── Async Task 1 (Forwarding Rule 1)
    └── Blocking Task (spawn_blocking)
        ├── Main logic: Run SSH process
        └── Standard Thread: Read PTY output
└── Async Task 2 (Forwarding Rule 2)
    └── Blocking Task (spawn_blocking)
        ├── Main logic: Run SSH process
        └── Standard Thread: Read PTY output
└── ... (More forwarding rules)
```

**Layer breakdown**:

1. **Layer 1 - Tokio Runtime Thread (Main Thread)**: Started by `#[tokio::main]`, manages the entire async runtime
2. **Layer 2 - Tokio Async Tasks**: One async task per forwarding rule, enabling concurrent management of multiple rules
3. **Layer 3 - Tokio Blocking Tasks (spawn_blocking)**: Executes blocking PTY operations in a thread pool to avoid blocking the async runtime
4. **Layer 4 - Standard Threads**: Standard threads created within blocking tasks to continuously read PTY output (since `portable-pty` uses blocking I/O)

This design ensures:
- Multiple forwarding rules can run concurrently
- Blocking PTY operations don't impact async runtime performance
- SSH process output is read and processed in a timely manner

### Tips

- **Prefer key-based authentication**: more secure and more reliable than storing a plaintext password
  - Use `ssh_key_path` to specify a private key file
  - Or use ssh-agent (when keys are loaded in ssh-agent, SSH will automatically use them without needing `ssh_key_path`)

### Security notes

- `ssh_password` is stored in **plaintext** in `config.toml`. Protect the file and its distribution accordingly.
- **Host key verification**: the tool uses `StrictHostKeyChecking=accept-new` by default, which auto-accepts new host keys but still validates existing ones. If a host key changes, the connection will be rejected to protect against MITM attacks.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
