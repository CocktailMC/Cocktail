<p align="center">
  <img src="logo.png" width="128" alt="Cocktail Manager">
</p>

<h1 align="center">Cocktail Manager</h1>

<p align="center">
  单机多实例的 Minecraft 控制面<br>
  <sub>Developer Preview · 版本由 git tag 自动驱动</sub>
</p>

<p align="center">
  <img src="https://ziadoua.github.io/m3-Markdown-Badges/badges/Rust/rust1.svg" alt="Rust" height="30">
  <img src="https://ziadoua.github.io/m3-Markdown-Badges/badges/React/react1.svg" alt="React" height="30">
  <img src="https://ziadoua.github.io/m3-Markdown-Badges/badges/ViteJS/vitejs1.svg" alt="Vite" height="30">
  <img src="https://ziadoua.github.io/m3-Markdown-Badges/badges/TypeScript/typescript1.svg" alt="TypeScript" height="30">
  <img src="https://ziadoua.github.io/m3-Markdown-Badges/badges/SQLite/sqlite1.svg" alt="SQLite" height="30">
  <img src="https://ziadoua.github.io/m3-Markdown-Badges/badges/Docker/docker1.svg" alt="Docker" height="30">
  <img src="https://ziadoua.github.io/m3-Markdown-Badges/badges/Java/java1.svg" alt="Java" height="30">
  <img src="https://ziadoua.github.io/m3-Markdown-Badges/badges/NodeJS/nodejs1.svg" alt="Node.js" height="30">
  <img src="https://ziadoua.github.io/m3-Markdown-Badges/badges/Linux/linux1.svg" alt="Linux" height="30">
  <img src="https://ziadoua.github.io/m3-Markdown-Badges/badges/Windows/windows1.svg" alt="Windows" height="30">
</p>

控制面用 Rust 管进程与 Docker；管理端是 React。第一次打开会初始化最高管理员，账号存在本机 SQLite。实例可热接管：控制面重启后，仍在跑的服务器会重新接上，不会误显示成已停止。

---

## 功能

| 实例 | 内容 | 运维 |
|:---|:---|:---|
| 启停 / 重启 / 优雅 `stop` | Paper / Vanilla 装核 | 定时备份、重启、指令 |
| CPU、内存、TPS、在线人数 | Modrinth / Hangar / Spigot | zip 备份与恢复 |
| 控制台 WebSocket | 插件启停、上传 | 计划任务 |
| JVM `-Xmx` 自动注入 | 世界导入导出、重置 | 崩溃 Webhook |
| `server.properties` 表单 | 玩家 kick / ban / op | 审计日志 |
| 端口冲突检测、EULA | 文件浏览与 512 MiB 上传 | 机群批量操作 |
| 本机进程或 Docker | | 重启后认回 PID / 容器 |
| 实例克隆（自动选端口） | 启动前预检（端口/磁盘/EULA） | 备份恢复预览（条目/世界/插件/level.dat） |
| 世界 zip 下载与上传 | MC 版本自动识别与比对 | 审计日志落 SQLite，可检索、按操作者过滤 |

Docker 运行时可设 `--memory` / `--cpus`。多节点 Agent 已具备基础能力（心跳、远程命令下发），但断线期间的命令 seq/ack 持久化、远端网络监控尚未实现。

---

## 开发启动

需要 **Rust**（stable）和 **Node.js 22+**。

```bash
# 控制面 — http://127.0.0.1:11011
cargo run -p cocktail-control

# 管理端 — http://127.0.0.1:5173（开发时代理 /api）
cd admin && npm install && npm run dev
```

首次打开管理端会进入最高管理员引导。之后用该账号登录。

角色分五档，写操作按路由鉴权，不再是「登录即可全权」：

| 角色 | 能力 |
|:---|:---|
| Owner | 全部，含用户与 Owner 管理 |
| 管理员 | 启停、控制台、文件、插件、玩家、备份、网络、节点；无用户管理 |
| 客服 | 启停、控制台、玩家、备份、网络 |
| 开发 | 控制台、文件、插件、启停 |
| 观察员 | 只读，所有写操作返回 403 |

会话 12 天过期，写请求需带 `X-Cocktail-CSRF`；同一用户名连续 5 次登录失败锁定 15 分钟。

### 安全特性

| 域 | 实现 |
|:---|:---|
| 密码哈希 | Argon2id + 随机盐；密码长度 8–128 字符 |
| 会话 | UUID 拼接 token，12 天 TTL；写请求强制 `X-Cocktail-CSRF` 头 |
| 2FA | TOTP（RFC 6238），±1 窗口；登录时代码校验用恒定时间比较 |
| 限流 | 同 IP+用户名 5 次/10 分钟；从 `ConnectInfo<SocketAddr>` 取真实对端，不信任 `X-Forwarded-For` |
| 凭据存储 | 后端 `HttpOnly; SameSite=Strict` Cookie；HTTPS 下自动 `Secure` |
| 随机数源 | 全部使用 `rand_core::OsRng`（包括主密钥、nonce、session token）；不依赖 PRNG 回退 |
| 加密 | ChaCha20-Poly1305；每部署独立 master.key + 每部署独立 salt |
| RBAC | 五档角色，未知角色回退最小权限集（仅 `view`），不 fail-open |
| 备份恢复 | snapshot 路径双重校验：组件级拒绝 `..`/绝对/盘符前缀 + canonicalize 后必须 starts_with dest_root |
| JRE 缓存 | 缓存命中时 `java -version` 探测实际版本，meta 与实际不符则重新部署 |
| 审计 | 写操作落 `data/audit.jsonl`，可按操作者过滤 |

### 可选环境变量

| 变量 | 说明 |
|:---|:---|
| `COCKTAIL_BIND` | 监听地址，默认 `0.0.0.0:11011`；非 loopback 时控制面启动会 warn 提示需前置 TLS 反代，生产建议显式设 `127.0.0.1:11011` |
| `COCKTAIL_API_TOKEN` | 机器 Token，供脚本调用（与登录并存）；命中后等同超管身份，等价于关闭 2FA，仅用于机器间调用 |
| `COCKTAIL_WEBHOOK_URL` | 全局崩溃 Webhook；也可在面板里覆盖 |
| `COCKTAIL_WEB_ROOT` | 生产环境 Admin 静态目录 |
| `COCKTAIL_CORS_ORIGINS` | 额外允许的跨域来源，逗号分隔；默认仅本机 5173/11011 |
| `COCKTAIL_PROXY` | 强制 HTTP 代理，例如 `http://127.0.0.1:7890` |
| `HTTPS_PROXY` | 标准代理变量；未设置时 Windows 会读取系统代理（Clash / IE） |
| `COCKTAIL_MASTER_KEY` | 加密主密钥：64 位 hex（32 字节）或任意字符串作为 passphrase。未设置时从 `data/master.key` 加载或随机生成 32 字节；passphrase 模式还会生成每部署独立的 `data/master.salt` |
| `COCKTAIL_PLANE` | Agent 连接主控的基地址，例如 `https://panel.example.com`；默认 `http://127.0.0.1:11011` |
| `COCKTAIL_NODE_TOKEN` | Agent 节点 Token，在控制面「节点」页创建节点时生成；传输应走 TLS（`COCKTAIL_PLANE=https://...`） |
| `COCKTAIL_PLUGIN_HOST` | .NET 插件宿主监听地址 |
| `COCKTAIL_PLUGIN_TOKEN` | .NET 插件宿主与控制面之间的认证 Token |
| `COCKTAIL_PLUGIN_DIR` | 插件目录，默认 `data/plugins` |
| `COCKTAIL_PLUGIN_AUTOSTART` | 插件宿主是否自启动，默认 `1` |

数据目录（相对工作目录）：

```
data/
  cocktail.db        # 管理员、会话、面板设置
  state.json         # 实例与计划任务
  master.key         # 加密主密钥（首次启动随机生成）
  master.salt        # passphrase 模式下的随机盐（每部署独立）
  audit.jsonl        # 审计日志（按操作追加）
  instances/<id>/    # 各服工作目录
    runtime/jre/     # 实例独立 JRE（按 java_major 部署，meta 校验实际版本）
  java/temurin-NN-jre/  # 全局 JRE 模板（按 major + image 类型组织）
  logs/              # 控制台与审计
  backups/
```

---

## 安装包

产物在 `dist/`。生产安装后访问 `http://127.0.0.1:11011`（二进制内嵌 Admin）。

### Linux（deb / rpm）

```bash
python3 scripts/package_linux.py

sudo dpkg -i dist/cocktail_0.1.0_amd64.deb
# 或
sudo rpm -Uvh dist/cocktail-0.1.0-1.x86_64.rpm
sudo systemctl start cocktail-control
```

依赖：`cargo`、`npm`。未安装 [nfpm](https://nfpm.goreleaser.com/) 时脚本会自行下载。

| | 路径 |
|:---|:---|
| 配置 | `/etc/cocktail/cocktail.env` |
| 数据 | `/var/lib/cocktail` |

### Windows（zip / msi）

```powershell
.\scripts\package-windows.ps1
```

会生成 `dist/cocktail-<ver>-windows-x64.zip`。双击 `Start-Cocktail.cmd` 启动控制面（控制台保留日志），浏览器打开 http://127.0.0.1:11011。便携包数据在 exe 旁的 `data\`；MSI 安装后数据在 `%ProgramData%\Cocktail`。

防火墙拉黑与立即踢掉 IPv4 连接需要**以管理员运行**控制面：规则写入 Windows 防火墙分组 Cocktail，踢连接走 IP Helper（`SetTcpEntry`）。IPv6 拦截靠防火墙规则。控制面重启后可认回仍在跑的 Java 进程，并通过命名管道继续向控制台写指令（`stop`、kick 等）。实例网络连接列表不依赖 Linux `ss`/`conntrack`。

若已安装 [WiX v3](https://wixtoolset.org/)（`candle` / `light` / `heat`），额外打出 MSI。

```powershell
winget install WiXToolset.WiXToolset
```

---

## 仓库结构

```
Cocktail/
  crates/cocktail-control   控制面（Axum + SQLite）
  admin/                    React 管理端
  packaging/                systemd / env / WiX
  scripts/                  deb、rpm、msi 打包
```

---

## 许可证

[Apache License 2.0](LICENSE) · Copyright 2026 [CocktailMC](https://github.com/CocktailMC)

徽章来自 [m3 Markdown Badges](https://github.com/ziadOUA/m3-Markdown-Badges)。
