# Cocktail Edge & Scroll · 产品事实

**核验日期**: 2026-09-12  
**来源**: 仓库 `README.md`、`项目方案.md` v1.0 草案、`admin/` 源码（非训练语料）

## 存在性与状态

- **产品名**: Cocktail Manager（控制面）/ 发行轨 **Cocktail Scroll**、**Cocktail Edge**
- **组织**: CocktailMC（https://github.com/CocktailMC），License Apache-2.0
- **当前版本**: v0.1 · 26Q3（已存在可运行的 Rust 控制面 + React 管理端）
- **Edge / Scroll**: 方案已定、**尚未作为独立发布通道落地**（Phase 1 目标 2026 Q4；第一个 Edge 月度版标记为 `edge-26.10`）
- **不是** 鸡尾酒酒吧网站、不是 GSAP cocktail landing 教程项目

## 产品是什么

单机多实例 Minecraft 控制面：Rust 管进程/Docker，React 管理端，本机 SQLite。实例热接管、Windows 网络防护（IP Helper / 防火墙分组）、中文代理环境适配是现有差异化。

## 双轨定义（设计必须说对的事实）

| | Cocktail Scroll | Cocktail Edge |
|---|---|---|
| 隐喻 | Arch 式滚动 · **流动** | Fedora/Manjaro 式月度凝固 · **凝固** |
| 发布 | main CI 全绿即出 | 每月从 Scroll 流选「最稳定」快照 |
| 版本号 | `scroll-<sha7>-<yyyymmdd>-<hhmm>` | `edge-<yyyy>.<mm>` |
| 例 | `scroll-a3f9c2b-20260912-1430` | `edge-26.10` |
| EOL | 7 天滚动窗口 | 2 个月（当月+上月） |
| 实验功能 | 低+中风险默认开 | 只开低风险 |
| 升级 | 自动；崩溃 3 次回滚 | 半自动（通知后确认） |
| 用户 | 开发者 / 尝鲜党 | 早期采用者 / 小服主 |
| 许可证 | Apache-2.0 永久免费 | Apache-2.0 永久免费 |

**一句话**: Scroll 是流动的，Edge 是凝固的。两者都是 Fedora 角色的更细拆分，**不锁功能**；付费墙在更后面的 PRO Stable / Enterprise（本设计不主推）。

## CLI（真实命令，禁止编造）

```
cocktail update --channel=scroll
cocktail update --channel=scroll --pin=scroll-a3f9c2b-20260912-1430
cocktail update --channel=scroll --rollback
cocktail update --channel=edge
```
