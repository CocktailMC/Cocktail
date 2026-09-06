<p align="center">
  <img src="logo.png" width="128" alt="Cocktail Manager">
</p>

<h1 align="center">Cocktail Manager</h1>

<p align="center">
  单机多实例的 Minecraft 控制面<br>
  <sub>v0.1 · 26Q3</sub>
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

Docker 运行时可设 `--memory` / `--cpus`。本机进程硬限、多节点 Agent 尚未提供。

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

可选环境变量：

| 变量 | 说明 |
|:---|:---|
| `COCKTAIL_BIND` | 监听地址，默认 `0.0.0.0:11011` |
| `COCKTAIL_API_TOKEN` | 机器 Token，供脚本调用（与登录并存） |
| `COCKTAIL_WEBHOOK_URL` | 全局崩溃 Webhook；也可在面板里覆盖 |
| `COCKTAIL_WEB_ROOT` | 生产环境 Admin 静态目录 |

数据目录（相对工作目录）：

```
data/
  cocktail.db      # 管理员、会话、面板设置
  state.json       # 实例与计划任务
  instances/       # 各服工作目录
  logs/            # 控制台与审计
  backups/
```

---

## 安装包

产物在 `dist/`。生产安装后访问 `http://127.0.0.1:11011`（二进制内嵌 Admin）。

### Linux（deb / rpm）

```bash
chmod +x scripts/package-linux.sh packaging/scripts/*.sh
./scripts/package-linux.sh

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

防火墙拉黑需要**以管理员运行**控制面（写入 Windows 防火墙分组 Cocktail）。控制面重启后可认回仍在跑的 Java 进程，但控制台指令依赖启动时的 stdin 管道，认回后无法再向该进程写指令（可在游戏内或下次启动后操作）。踢连接依赖 Linux `ss`/`conntrack`，Windows 上靠防火墙规则与游戏 ban-ip。

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
