## ssh-tunnel-manager

[English](README.md) | 简体中文

一个用于**管理多个长期运行的 SSH 本地端口转发**的工具：从 `config.toml` 读取多条规则，每条规则运行一个持久的 `ssh -N -L ...` 进程，支持自动重连。

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

### 运行

在项目根目录放好 `config.toml` 后运行：

```bash
cargo run --release
```

也可以本地安装后使用：

```bash
cargo install --path .
ssh-tunnel-manager
```

### 配置（`config.toml`）

配置文件结构：

- `[[forwarding]]`：一条转发规则（可写多条）
- **local_bind**：本地监听地址（可选，默认 `127.0.0.1`）
- **local_port**：本地监听端口（必填）
- **remote_address**：远端目标 `host:port`（必填，支持 `[ipv6]:port`）
- **ssh_host**：SSH 目标（host/IP，或 `~/.ssh/config` 里的 Host alias）
- **ssh_port**：SSH 端口（可选，默认 `22`）
- **ssh_user**：SSH 用户名（必填）
- **ssh_key_path**：私钥路径（可选，推荐；支持 `~`）
- **ssh_password**：密码（可选；PTY 会自动响应密码/passphrase 提示）
- **ssh_extra_args**：额外透传给 `ssh` 的参数数组（可选）

示例请看 `config.toml.example`。

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

- **首次连接不阻塞（推荐）**：避免首次连接卡在 host key 确认提示，可加：
  - `ssh_extra_args = ["-o", "StrictHostKeyChecking=accept-new"]`
- **优先使用密钥认证**：比在配置文件里保存明文密码更安全、也更稳定
  - 使用 `ssh_key_path` 配置私钥路径
  - 或使用 ssh-agent（密钥已加载到 ssh-agent 时，SSH 会自动使用，无需配置 `ssh_key_path`）

### 安全提示

- `ssh_password` 是**明文**保存在 `config.toml` 中，请自行控制文件权限与分发方式。
- **Host key 确认**：本工具默认**不会自动回复** `Are you sure you want to continue connecting (yes/no/[fingerprint])?` 提示。
  - **原因**：自动接受未知 host key 存在安全风险（可能绕过 SSH 的中间人攻击防护）
  - **解决方案**：
    1. 使用 `ssh_extra_args = ["-o", "StrictHostKeyChecking=accept-new"]`（推荐，自动接受新 host key 但会验证）
    2. 预先手动添加 host key 到 `~/.ssh/known_hosts`（最安全）
  - 如果检测到 host key 确认提示，工具会终止连接并提示用户配置上述选项。

## 许可证

本项目采用 MIT 许可证 - 详见 [LICENSE](LICENSE) 文件。

