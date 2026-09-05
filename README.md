# Cocktail Manager

新一代通用 Minecraft 服务器控制面 · **v0.1 / 26Q3**

栈：**Rust**（Axum 控制面）+ **React**（Vite Admin）

## 快速启动

```powershell
# 可选：生产务必设置
$env:COCKTAIL_API_TOKEN = "change-me"
$env:COCKTAIL_WEBHOOK_URL = "https://hooks.example/crash"

cargo run -p cocktail-control

cd admin
npm install
npm run dev
```

- 控制面：http://127.0.0.1:11011  
- Admin：http://127.0.0.1:5173  

## 能力对照（相对缺口清单）

| # | 项 | 状态 |
|---|---|---|
| 1 | 真实进程指标 | **已做** sysinfo CPU/RSS + 日志解析 TPS/玩家 |
| 2 | 优雅停止 | **已做** stdin `stop` + 30s 超时强杀 |
| 3 | EULA 流程 | **已做** `eula_accepted` / `POST /eula` / UI |
| 4 | JVM `-Xmx` 注入 | **已做** Java 启动自动注入 |
| 5 | 端口联动 properties | **已做** + 端口冲突检测 |
| 6 | 认证 | **已做** `COCKTAIL_API_TOKEN` Bearer（未设置则开放，仅开发） |
| 7 | 上传/下载 | **已做** multipart 上传 + 二进制下载 |
| 8 | 备份压缩 | **已做** zip |
| 9 | 日志持久化 | **已做** `data/logs/{id}.log` |
| 10 | 删除备份 | **已做** |
| 11 | 定时任务 | **已做** schedules API（backup/restart/command） |
| 12 | 审计日志 | **已做** `data/audit.jsonl` |
| 13 | 版本 jar 下载 | **已做** Paper Fill v3 + Vanilla Mojang |
| 14 | 插件管理 | **已做** 列表/启用禁用/上传 |
| 15 | 玩家管理 | **已做** list + kick/ban/op/deop |
| 16 | 世界管理 | **已做** 列表/重置/导出/导入 |
| 17 | properties 表单 | **已做** `/properties` + 配置 Tab |
| 18 | 崩溃 Webhook | **已做** `COCKTAIL_WEBHOOK_URL` / 实例 webhook |
| 19 | cgroup / 容器硬限 | **部分** Docker `--memory/--cpus`（本机进程尚无 Job Object） |
| 20 | 多节点 Agent | 未做（单机多实例 + 机群批量已做） |
| 21 | backup created_at | **已修** 文件系统时间 |
| 22 | Protobuf | 未做（仍 REST） |

## 打包（deb / rpm / msi）

一次性产物目录：`dist/`。

### Linux（deb + rpm）

在 Linux 或 WSL 中：

```bash
chmod +x scripts/package-linux.sh packaging/scripts/*.sh
./scripts/package-linux.sh
```

依赖：`cargo`、`npm`；脚本会自动下载 [nfpm](https://nfpm.goreleaser.com/)（若未安装）。

安装示例：

```bash
sudo dpkg -i dist/cocktail_0.1.0_amd64.deb
# 或
sudo rpm -Uvh dist/cocktail-0.1.0-1.x86_64.rpm
sudo systemctl start cocktail-control
```

默认：`http://127.0.0.1:11011`（二进制 + 内嵌 Admin UI）。  
配置：`/etc/cocktail/cocktail.env`，数据：`/var/lib/cocktail`。

### Windows（msi + zip）

```powershell
.\scripts\package-windows.ps1
```

- 始终生成便携包：`dist/cocktail-<ver>-windows-x64.zip`
- 若已安装 [WiX Toolset v3](https://wixtoolset.org/)（`candle` / `light` / `heat`），额外生成 MSI：`dist/cocktail-<ver>-windows-x64.msi`

```powershell
winget install WiXToolset.WiXToolset
```

### 环境变量（打包运行）

| 变量 | 说明 |
|---|---|
| `COCKTAIL_BIND` | 监听地址，默认 `0.0.0.0:11011` |
| `COCKTAIL_WEB_ROOT` | Admin 静态目录（含 `index.html`） |
| `COCKTAIL_API_TOKEN` | 可选 Bearer 鉴权 |
| `COCKTAIL_WEBHOOK_URL` | 可选崩溃 Webhook |
