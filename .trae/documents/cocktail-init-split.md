# Cocktail-init 拆分方案

## Context

当前 Cocktail 所有功能集中在 `crates/cocktail-control` 一个二进制里。用户决定把"用户空间操作"独立成 `crates/cocktail-init`，参考 systemd --user 的用户态设计哲学：

- **cocktail-init 是 control 的子进程**：control 启动时 fork+exec 拉起 init；control 退出时 init 跟着退；init 崩溃 control 可重启它
- **init 是用户态进程**：不需要 root/Administrator 特权，以当前登录用户身份运行；不装 systemd unit，不装 Windows Service；不依赖 PID 1 / 系统级 init
- **设计模型**：类似 nginx master-worker 或 Chrome 多进程——control 是入口（路由/鉴权/状态/节点协议/插件桥），init 是用户数据操作执行者（实例文件、JRE、备份、运行时进程、容器、压缩、Java 探测）

目标：物理隔离用户数据操作与控制面，提升安全边界；init 崩溃不影响 control 鉴权与路由能力。

## 关键决策：crypto / secrets / totp 归属

| 模块 | 归属 | 理由 |
|---|---|---|
| **crypto** | 全留 control | 纯计算无 IO；ct_eq/sha256/hmac/hkdf/aead 是热路径原语，搬走等于每次鉴权套 IPC 延迟 |
| **totp** | 全留 control | verify_code 是登录热路径，只依赖 crypto::ct_eq，无文件 IO |
| **secrets** | **拆双面** | 见下 |

### secrets 拆双面

- **init 端**：`master.key` / `master.salt` 文件的生成、写入、权限设置、轮换。暴露 `get_master_key()` RPC（control 启动握手时调一次）和 `rotate_master_key()` RPC。
- **control 端**：保留 `encrypt/decrypt/encrypt_bytes/decrypt_bytes` 函数体不变，把 `master_key()` 改成：
  1. `OnceLock::get_or_init` 里先调 `InitClient::get_master_key()`
  2. IPC 不可达时 fallback 直接读 `data/master.key`（兼容老部署、单进程、init 故障降级）
- **绝不让** encrypt/decrypt 本身走 IPC——state.rs 的 bearer_ok/token_role、instance spec 序列化都可能触发解密，IPC 化引入 N 倍延迟。

核心原则：**安全原语归属 control，密钥材料管理归属 init，运行时握手拉取 + 文件 fallback**。

## IPC 通道

**父子进程模型下用 stdin/stdout pipe + 长度前缀 JSON-RPC 2.0**：

- control 用 `tokio::process::Command::new("cocktail-init")` 启动子进程，`stdin`/`stdout` piped
- 双向通信：control→init 走子进程 stdin；init→control 走子进程 stdout；日志走 stderr（control 转发到自己的 tracing）
- 协议：length-prefixed JSON-RPC 2.0（4 字节 big-endian length + JSON body），双向（方法调用 + 事件推送）
- 事件流（InstanceEvent/LogLine/MetricSample）通过 stdout 主动推送，control 端 fan-out 到 WebSocket 订阅者
- 不需要 `interprocess` crate，不需要 UDS / Named Pipe，不需要端口

**否决方案**：
- 本地 HTTP：明文监听易被同机进程访问，需额外 ACL token
- UDS/Named Pipe：父子进程下没必要，stdin/stdout 更简单
- 系统服务（systemd unit / Windows Service）：违背"用户态进程"原则

## RPC 接口（按功能分组）

- **生命周期**：start_instance / stop_instance / restart_instance / send_command / get_instance_status / adopt_running
- **文件**：list_files / read_file / write_file / delete_file / move_file / import_archive（流式 chunk）
- **归档**：extract_archive / create_archive
- **容器**：docker_status / list_images / pull_image / spawn_docker_instance
- **Java**：list_jre / ensure_jre / remove_jre
- **Versions**：list_versions / install_version / search_modrinth / install_modrinth / search_hangar / search_spiget
- **Backup**：create_backup / restore_backup / list_backups / delete_backup
- **7z**：ensure_7z / extract_7z / create_7z
- **RCON**：try_rcon / exec_rcon
- **Secrets**：get_master_key（握手一次）/ rotate_master_key
- **事件订阅**：subscribe_events（init → control 推送 InstanceEvent / LogLine / MetricSample）

## 进程生命周期管理

- control 的 `run_plane()` 启动时调 `InitClient::spawn()`：fork+exec cocktail-init，建立 stdin/stdout pipe，发起 Hello 握手（含 control 版本号、init 版本号校验）
- control 退出时调 `InitClient::shutdown()`：发 JSON-RPC `shutdown`，等 init 退出；超时则 SIGTERM/Kill
- init 崩溃时 control 的 `Child::try_wait()` 检测到，记日志，可选自动重启（带指数退避，最多 3 次）
- control 端的 `InitClient` 用 `tokio::select!` 同时读 stdout（RPC 响应 + 事件）和监听 child 退出

## 拆分范围

**搬到 cocktail-init**：
- `instance/files.rs`、`archive.rs`、`process.rs`、`runtime.rs`、`container.rs`、`versions.rs`、`registry.rs`（registry.rs 81KB 需后续阶段拆分为多模块）
- `java.rs`、`backup.rs`、`sevenz.rs`、`rcon.rs`
- `secrets.rs` 的密钥文件管理部分（encrypt/decrypt 函数体留 control）

**留在 cocktail-control**：
- `api/`（路由+handler）、`auth.rs`、`state.rs`、`db.rs`、`cluster.rs`、`agent_runtime.rs`
- `crypto.rs`、`totp.rs`、`secrets.rs`（加解密函数 + 握手 + fallback）
- `plugin_bridge.rs`、`http.rs`、`platform.rs`、`metrics.rs`、`i18n.rs`、`openapi.rs`
- `hostnet.rs`、`hardening.rs`、`diagnostics.rs`、`identity.rs`、`rbac.rs`

## 分阶段实施路线图

### 阶段 1：骨架 + secrets 拆分 + 最小 IPC 验证
- 新建 `crates/cocktail-init` crate（bin），加入 workspace.members
- 实现 IPC：tokio::process::Command + stdin/stdout pipe + length-prefixed JSON-RPC dispatcher
- 实现 `InitClient`：spawn child、Hello 握手、call/response、event stream、shutdown；fallback：IPC 失败时退化到本地函数调用（保证旧部署单进程可跑）
- secrets 拆双面：master.key/salt 文件管理搬 init，`get_master_key` RPC，control 端 `master_key()` 改握手拉取 + 文件 fallback
- 搬 2 个最简单功能验证管线：`ensure_7z`、`try_rcon`
- **关键文件**：
  - `Cargo.toml`（workspace.members + 依赖）
  - `crates/cocktail-init/Cargo.toml`、`crates/cocktail-init/src/{main.rs,server.rs,proto.rs}`
  - `crates/cocktail-control/src/init_client.rs`（新文件，封装 IPC client + fallback）
  - `crates/cocktail-control/src/secrets.rs`（master_key() 改握手 + fallback）
  - `crates/cocktail-control/src/lib.rs`（run_plane 启动时拉起 init）
- **风险**：IPC 协议定型失误会返工；secrets fallback 路径需充分测试；child 生命周期与 tokio runtime 协调

### 阶段 2：用户空间 IO
- 搬 `instance/files`、`archive`、`sevenz`、`java`、`versions`、`registry`、`modrinth`、`hangar`、`spiget`
- RPC 增加 streaming chunk 模式（大文件/归档）+ 进度事件
- **风险**：流式背压；registry.rs 需重构为多模块

### 阶段 3：进程 + 容器
- 搬 `instance/process`、`container`、`runtime`
- ProcessHandle 的 mpsc 跨进程桥接：init 持有 child + cmd_tx/stop_tx，通过事件 stream 把 LogLine/InstanceEvent 推回 control
- **风险**：进程归属语义变化（child 由 init 派生，control 只观察）；reattach 逻辑跨进程

### 阶段 4：backup + 收尾
- 搬 `backup` 存储层
- `rotate_master_key` RPC：init 生成新 key + 通知 control，control 重加密敏感字段
- 打包脚本扩展：package_linux.py + package-windows.ps1 增加 cocktail-init 产物
- CI 工作流扩展：dt-release.yml build cocktail-init
- **风险**：密钥轮换事务性；backup 重加密中途失败回滚

## 复用与不重写

- 复用 `tokio::process::Command` + `tokio::io::AsyncReadExt/AsyncWriteExt`，不自实现 pipe FFI（替代现有 `stdin_bridge.rs` 的 Windows-only `CreateNamedPipeW`）
- 复用 workspace 已有 `tokio` / `serde_json`
- 搬移的模块保留原有 `#[cfg(test)]` 单元测试，纯函数测试不变照跑
- `secrets.rs` 的 `encrypt/decrypt` 函数体不变，只改 `master_key()` 实现
- `crypto.rs`、`totp.rs` 完全不动

## 验证（阶段 1）

1. `cargo build -p cocktail-init -p cocktail-control` 编译通过
2. `cargo test -p cocktail-init -p cocktail-control` 全过（含原有 secrets/crypto/totp 单测）
3. 端到端：control `run_plane()` 启动 → fork+exec init → Hello 握手成功 → control 调 `get_master_key` → 解密一个 `enc:v1:` 字段成功
4. fallback：杀掉 init 子进程 → control 启动仍能从 `data/master.key` 加载密钥并鉴权
5. `cargo clippy -p cocktail-init -p cocktail-control --all-targets` 无 error

## 不做的事

- 不装 systemd unit / Windows Service（init 是用户态进程）
- 不动 crypto.rs / totp.rs（这两个文件原封不动）
- 不动 auth.rs / state.rs 的鉴权路径（secrets 内部 fallback 透明）
- 不创建 README 或文档文件（除本 plan 文件外）
- 不修改 git config
- 阶段 1 不动打包脚本和 CI（先把 crate 立起来再谈发布）
