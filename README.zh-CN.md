## stm

[English](README.md) | 简体中文

一个用于**管理多个长期运行的 SSH 端口转发**的工具：从 `config.toml` 读取多条规则，每条规则运行一个持久的 `ssh -N -L/-R ...` 进程，支持自动重连，支持单个 SSH 连接转发多个端口。

### 为什么需要这个工具？

SSH 本身可以通过 keepalive 参数（`ServerAliveInterval`、`ServerAliveCountMax`、`TCPKeepAlive`）检测断开并退出，但 **SSH 无法自动重启**。如果你只是手动运行 `ssh -N -L ...`，每次断开都需要手动重启。

本工具的核心价值是**自动重启**：当 SSH 因网络问题退出时自动重连，并智能处理认证失败（停止重试避免日志刷屏）。详见下方"特性"部分。

### 特性

- **多规则并发**：每条 `[[forwarding]]` 启动一个独立的 SSH 隧道
- **自动重连**：非认证类失败会带退避重试
- **认证失败不重试**：检测到 `Permission denied` / `Authentication failed` 时，该规则直接停止（避免刷屏）
- **退出行为**：按 `Ctrl-C` 退出；或当所有规则都结束时自动退出

### 依赖

- **系统 `ssh`**：确保命令行可执行 `ssh`
  - macOS/Linux：通常自带
  - Windows：请安装 OpenSSH（或确保 `ssh.exe` 在 `PATH` 里）

### 安装

```bash
# 从 GitHub 安装
cargo install --git https://github.com/miiy/ssh-tunnel-manager.git

# 或从本地源码安装
cargo install --path .
```

安装后运行：

```bash
stm                    # 使用当前目录下的 config.toml
stm /path/to/config    # 使用指定配置文件
```

### 配置（`config.toml`）

配置文件结构：

- `[[forwarding]]`：一条转发规则（可写多条）
  - **forwards**：端口转发映射数组（必填）
    - **mode**：`"local"`（-L）或 `"remote"`（-R）（可选，默认 `"local"`）
    - **local_address**：本地绑定地址和端口，如 `"127.0.0.1:3316"` 或 `"0.0.0.0:8080"`（必填）
    - **remote_address**：远端目标 `host:port`（必填，支持 `[ipv6]:port`）
  - **ssh_host**：SSH 目标（host/IP，或 `~/.ssh/config` 里的 Host alias）
  - **ssh_port**：SSH 端口（可选，默认 `22`）
  - **ssh_user**：SSH 用户名（必填）
  - **ssh_key_path**：私钥路径（可选，推荐；支持 `~`）
  - **ssh_password**：密码（可选；PTY 会自动响应密码/passphrase 提示）
  - **server_alive_interval**：SSH `ServerAliveInterval` 秒数（可选，默认 `60`）
  - **server_alive_count_max**：SSH `ServerAliveCountMax`（可选，默认 `3`）
  - **connect_timeout**：SSH `ConnectTimeout` 秒数（可选，默认 `10`）
  - **ssh_extra_args**：额外透传给 `ssh` 的参数数组（可选）

示例：

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

### 架构设计

本工具采用多层线程嵌套架构，以在保持异步并发的同时处理阻塞的 PTY 操作：

```
主线程 (Tokio Runtime)
└── 异步任务 1 (转发规则 1)
    └── 阻塞任务 (spawn_blocking)
        ├── 主逻辑：运行 SSH 进程
        └── 标准线程：读取 PTY 输出
└── 异步任务 2 (转发规则 2)
    └── 阻塞任务 (spawn_blocking)
        ├── 主逻辑：运行 SSH 进程
        └── 标准线程：读取 PTY 输出
└── ... (更多转发规则)
```

**层次说明**：

1. **第1层 - Tokio 运行时线程（主线程）**：由 `#[tokio::main]` 启动，管理整个异步运行时
2. **第2层 - Tokio 异步任务**：为每个转发规则创建一个异步任务，实现并发管理多个转发规则
3. **第3层 - Tokio 阻塞任务（spawn_blocking）**：将阻塞的 PTY 操作放到线程池中执行，避免阻塞异步运行时
4. **第4层 - 标准线程**：在阻塞任务内部创建标准线程，用于持续读取 PTY 输出（因为 `portable-pty` 使用阻塞 I/O）

这种设计确保了：
- 多个转发规则可以并发运行
- 阻塞的 PTY 操作不会影响异步运行时的性能
- SSH 进程的输出能够被及时读取和处理

### 常见建议

- **优先使用密钥认证**：比在配置文件里保存明文密码更安全、也更稳定
  - 使用 `ssh_key_path` 配置私钥路径
  - 或使用 ssh-agent（密钥已加载到 ssh-agent 时，SSH 会自动使用，无需配置 `ssh_key_path`）

### 安全提示

- `ssh_password` 是**明文**保存在 `config.toml` 中，请自行控制文件权限与分发方式。
- **Host key 验证**：工具默认使用 `StrictHostKeyChecking=accept-new`，会自动接受新 host key，但已有 host key 变更时会拒绝连接，防止中间人攻击。

## 许可证

本项目采用 MIT 许可证 - 详见 [LICENSE](LICENSE) 文件。
