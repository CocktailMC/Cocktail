# Cocktail Edge & Scroll · 三方向共同设计 Spec

**产出**: 网页落地页 / 通道介绍页（不是 PPT、不是 Admin 设置面板、不是生产 Web App）  
**视口**: 截图统一 **1440×900**；页面可纵向滚动，首屏必须在 900px 高内读得懂  
**交付**: 单文件 HTML（纯 HTML/CSS，可少量 JS 做轨切换）；logo 必须 base64 内嵌  
**语言**: 中文正文 + 英文产品名（Cocktail / Scroll / Edge）；中文引号用「」  
**日期假设**: 2026-09-12，示例版本 `scroll-a3f9c2b-20260912-1430` 与 `edge-26.10`

---

## Assumptions（用户未口头确认，标在此）

1. **产出形态**：`cocktailmc.dev/channels` 式的双轨介绍落地页——让服主 5 秒内选 Scroll 还是 Edge，并带走一条真实 CLI。
2. **不主推 PRO/Enterprise**：可在页脚或淡化第三轨暗示「再往后才是订阅」，不抢主角。
3. **文案来自 `项目方案.md`**，不编造功能、价格、用户数。
4. 若用户其实要的是 Admin 内「切换通道」设置屏或路演 PPT，选定方向后改形态，不在三版里混。

---

## 产品 / 项目是什么

Cocktail Manager 是 CocktailMC 的 Minecraft 控制面（Rust + React，v0.1）。**Cocktail Scroll** 与 **Cocktail Edge** 是同一份 Apache-2.0 代码的两条免费发布轨，不是两个产品、不是功能锁：

- **Scroll = 流动**：每次 main CI 全绿出一版，版本号绑 git sha 与时间戳，7 天二进制窗口，自动升级，崩溃 3 次自动回滚，低+中风险实验默认开。给开发者和尝鲜党。
- **Edge = 凝固**：每月从过去 30 天 Scroll 流里挑「最稳定」的 commit 装瓶，版本号 `edge-yyyy.mm`，EOL 两个月，只开低风险实验，升级要确认。给早期采用者和小服主。

核心金句必须出现：**Scroll 是流动的，Edge 是凝固的。**

---

## 目标受众与使用场景

- **主受众**：中文 Minecraft 服主（Windows 单机/小机群很常见）、会跑 Paper/Vanilla、在意别把生产服滚坏。
- **次受众**：给 Cocktail 提 PR 的贡献者、Arch/Fedora 思维的 Linux 用户（方案明确借鉴 Arch 滚动 + Fedora/Manjaro 月度）。
- **场景**：第一次听说双轨、从 GitHub Releases 下来、对照 `cocktail update --channel=` 该填什么。
- **观看距离**：笔记本 1m，不是 10m 投屏，也不是手机 10cm 优先（可自适应但不做 App 原型）。
- **情感基调**：专业、可追溯、有调酒师工艺，不轻浮成酒吧菜单，不冷成 Azure 文档。Scroll 侧温度偏兴奋/流动；Edge 侧偏冷静/权威；整页是同一间酒厂的两个 vintage，不是两个无关品牌。

气质关键词：**流动 / 凝固 / 装瓶 / 可追溯 / 月度 vintage / 调酒师工具**。

---

## 核心信息与内容板块（三版必须同内容，只换设计）

按优先级，每版都要有，禁止 Lorem、禁止编造 stats：

1. **首屏 hero**  
   - 真 logo（马提尼杯）  
   - 产品名 Cocktail  
   - 金句「Scroll 是流动的，Edge 是凝固的」  
   - 各一条人话：Scroll = 永远最新但已构建；Edge = 月度装瓶的稳定快照  
   - 两个主 CTA：`选 Scroll` / `选 Edge`（可滚到对应区或切换）

2. **双轨对照**（必须结构化，不能只靠形容词）  

   | | Scroll | Edge |
   |---|---|---|
   | 隐喻 | Arch 式滚动 | Fedora/Manjaro 式凝固 |
   | 节奏 | CI 全绿即出（日均 1–3，方案预期） | 每月 1 次 |
   | 版本 | `scroll-a3f9c2b-20260912-1430` | `edge-26.10` |
   | EOL | 7 天 | 2 个月 |
   | 实验 | 低+中风险默认开 | 只开低风险 |
   | 升级 | 自动；崩溃 3 次回滚 | 通知后确认 |
   | 给谁 | 开发者 / 尝鲜党 | 小服主 / 早期采用者 |

3. **真实 CLI 块**（等宽，可复制气质，命令必须一字不差）  
   `cocktail update --channel=scroll`  
   `cocktail update --channel=scroll --pin=scroll-a3f9c2b-20260912-1430`  
   `cocktail update --channel=scroll --rollback`  
   `cocktail update --channel=edge`

4. **「为什么不是锁功能」** 一小段：main / Scroll / Edge 都是 Apache-2.0 完整功能；付费的是更后面的稳定承诺与商标，本页不卖 PRO。

5. **页脚**：Apache-2.0 · CocktailMC · 2026 · 第一个 Edge 目标 `edge-26.10`

禁止：假用户数、假评分、假「已有 10 万服主」、emoji 当图标、紫渐变、Inter 作 display、GitHub-dark+#64FFDA 偷懒、手画第二只杯子代替 logo。

---

## 视觉母题（form 必须从这里长出来）

这个主题别人套不到的结构：**同一只杯子里，酒液在流，杯体是固体。**  
Logo 已经是这个隐喻——奶油色液体（Scroll）盛在冰蓝玻璃结构（Edge）里，青柠是唯一的活点。

- Scroll 的 form：倾斜、连续条带、sha 字符流、未封口、时间戳在动  
- Edge 的 form：水平切线、月份标签、封口、网格凝固、`26.10` 像酒标 vintage  

**form 来自内容的哪里**：来自方案金句「流动 vs 凝固」+ 真实 logo 的液/杯结构，不是来自「科技落地页模板」。

色彩论证：奶油 `#FCF0D8` 与冰蓝 `#B4D8F0` 取自 logo 像素；青柠 `#6CC054` 只做小面积点睛；Edge 可用现有 Admin 海军 `#0F4C81` 作凝固侧结构色。不要发明科技紫。

---

## 输出格式与尺寸（三版统一）

- 单文件 `.html`，双击 `file://` 能开  
- 设计宽按 1440，首屏高 900；正文 ≥16px（中文建议 17–18），标签 ≥12px，正文对比度 ≥4.5:1  
- Logo：把 `assets/logo-b64.txt` 写成 `<img src="data:image/png;base64,...">`  
- 存盘：`d:\Cocktail\design\edge-scroll\design-demos\<逻辑名>.html`  
- 截图：1440×900 首屏 PNG 放到 `design-demos/`

## 三版布局骨架必须互异（硬）

由各 agent 的逻辑段指定，禁止换皮。

## 技术

纯 HTML/CSS；轨切换可用最少 JS。Google Fonts 允许。`text-wrap: pretty`。不要 React/Tailwind CDN 整站。不要 stock 灵感图。
