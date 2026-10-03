# Cocktail Manager WEBUI · 产品事实

**核验日期**: 2026-09-13  
**来源**: 仓库 `README.md`、`项目方案.md` v1.0 草案、`admin/` 源码（非训练语料）

## 存在性与状态

- **产品名**: Cocktail Manager（控制面管理端）
- **组织**: CocktailMC（https://github.com/CocktailMC），License Apache-2.0
- **当前版本**: v0.1 · 26Q3（Rust 控制面 + React 管理端，可运行）
- **访问**: 开发 `http://127.0.0.1:5173`；生产随控制面 `http://127.0.0.1:11011`
- **不是** 鸡尾酒酒吧网站、不是 Edge/Scroll 营销落地页、不是通用云厂商 Console

## 产品是什么

单机多实例 Minecraft 控制面：Rust 管进程/Docker，React 管理端，本机 SQLite。实例热接管、Windows 网络防护（IP Helper / 防火墙分组 Cocktail）、中文代理环境适配是现有差异化。

## 现有 WEBUI 信息架构（设计必须覆盖，不得发明不存在的模块）

**机群级（未选中实例）**

- 主界面：事件中心、健康一览、实例总数/运行中/已停止/异常、控制面状态、Docker 可用性、插件源（Modrinth / Hangar / Spiget）、服务器列表（启停重启、批量、进入）
- 全局网络、事件中心、用户权限、服务器设置、节点/Agent、扩展中心、审计日志
- 创建实例、EULA 确认、登录 / 首次最高管理员引导

**实例级（选中后 subnav）**

- 概览：仪表盘、服务器控制、玩家中心、自动化、网络、控制台
- 内容：服务端配置、插件/模组、文件、世界、备份策略
- 运维：计划任务、版本/jar、系统设置

**仪表盘真实指标（禁止编造新产品指标）**

状态 / pid / 已接管 / 节点 · 健康度（规则评分非 AI）· CPU · 在线玩家 · 内存 · TPS · MSPT · 上下行 · 实体/区块 · JVM 堆/GC · 网络 TCP 连接与 status ping

## 差异化（WEBUI 必须看得见，不能做成通用面板）

1. Windows 网络防护：防火墙分组 Cocktail、IP Helper 踢连接、按游戏端口套接字统计
2. 热接管：控制面重启后仍在跑的 Java 进程显示「已接管」，不是已停止
3. 中文环境：Modrinth/Hangar/Spiget 装核装插件走系统代理
4. 双轨：chrome 可标当前通道 `scroll-<sha7>-<date>` 或 `edge-26.10`，不是功能锁

## CLI（若出现必须一字不差）

```
cocktail update --channel=scroll
cocktail update --channel=edge
```
