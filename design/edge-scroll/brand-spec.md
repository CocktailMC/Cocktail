# Cocktail Brand Spec（Edge & Scroll 设计用）

**核验日期**: 2026-09-12  
**来源**: `logo.png`、`admin/public/favicon.svg`、`admin/src/index.css`、`admin/src/App.css`

## Logo

- 官方 mark：马提尼杯 + 奶油色酒液 + 冰蓝杯梗 + 青柠片（透明底裁切后）
- 路径：`design/edge-scroll/assets/logo-glass.png`
- Base64：`design/edge-scroll/assets/logo-b64.txt`（单文件 HTML 必须内嵌）
- Favicon 字标：深底 `#0c1210` + 薄荷绿 `#3dcf8e` 的「CM」几何字
- **禁区**：不要用 CSS/SVG 手画另一只杯子替代真 logo；不要把杯子画成 neon SaaS 图标

## 从资产抽出的色（不是临场发明）

从 logo 像素采样：

| 角色 | 色 | 论证 |
|---|---|---|
| 酒液奶油 | `#FCF0D8` / `#F0E4C0` | logo 杯中液体，Scroll「流动」 |
| 杯梗冰蓝 | `#B4D8F0` | logo 杯体结构，Edge「容器/凝固」 |
| 青柠 | `#6CC054` / 深边 `#24780C` | logo 唯一高饱和点睛，Scroll 酸度/活 |
| 虚空底 | `#0A0A0A` | logo 原透明/黑场 |
| 管理端海军 | `#0F4C81` | 现有 Admin `--primary`，Edge 冷静侧 |
| Favicon 薄荷 | `#3DCF8E` | 现有字标 accent，小面积点睛 |
| 管理端侧栏 | `#1A222C` | 现有 chrome |

收敛：**2–3 个有彩色** = 酒液奶油 + 杯梗冰蓝 + 青柠点睛；中性 = `#0A0A0A` → `#F7F4EC`。  
Admin 海军只给 Edge 轨用，不铺成整站科技蓝。禁止紫渐变 / GitHub-dark 霓虹。

## 字体

现有产品：`IBM Plex Sans` + `Noto Sans SC` + `IBM Plex Mono`。  
各方向可按风格库换 display，但中文必须带 Noto Sans SC / Source Han 链。代码/版本号用等宽（Plex Mono 或 Geist Mono）。

## 气质

调酒师工具，不是酒吧菜单，也不是通用 DevOps SaaS。关键词：流动 vs 凝固、装瓶、sha 可追溯、月度 vintage。徽章体系已有 Distiller / Mixer / Sommelier——可作点缀，不要堆 emoji。

## UI 截图

本页主角是**发行轨**不是管理面板。管理端 UI 截图非内容必需（去掉不损失「Scroll 流动 / Edge 凝固」信息）。产品识别靠 **真 logo + 真实 CLI + 真实版本号格式**。
