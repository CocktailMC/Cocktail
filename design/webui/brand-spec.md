# Cocktail Brand Spec（WEBUI 设计用）

**核验日期**: 2026-09-13  
**来源**: `logo.png`、`admin/public/favicon.svg`、`admin/src/index.css`、`admin/src/App.css`

## Logo

- 官方 mark：马提尼杯 + 奶油色酒液 + 冰蓝杯梗 + 青柠片
- 路径：`design/webui/assets/logo-glass.png`
- Base64：`design/webui/assets/logo-b64.txt`（单文件 HTML 必须内嵌）
- **禁区**：不要用 CSS/SVG 手画另一只杯子；不要 neon SaaS 图标杯

## 从资产抽出的色（禁止临场发明科技紫）

| 角色 | 色 | 论证 |
|---|---|---|
| 酒液奶油 | `#FCF0D8` / `#F0E4C0` | logo 杯中液体 |
| 杯梗冰蓝 | `#B4D8F0` | logo 杯体结构 |
| 青柠 | `#6CC054` / 深边 `#24780C` | logo 唯一高饱和点睛 = 运行中/活 |
| 虚空底 | `#0A0A0A` | logo 黑场 |
| 管理端海军 | `#0F4C81` | 现有 Admin `--primary` |
| Favicon 薄荷 | `#3DCF8E` | 现有字标 accent，小面积 |
| 管理端侧栏 | `#1A222C` / `#15202B` | 现有 chrome |

收敛：2–3 个有彩色 = 奶油 + 冰蓝 + 青柠点睛。崩溃用现有 `--danger` `#EF4444` 仅状态，不铺成品牌色。  
禁止紫渐变、GitHub-dark `#0D1117` + 通用青霓虹、Inter 作 display。

## 字体

现有产品：`IBM Plex Sans` + `Noto Sans SC` + `IBM Plex Mono`。  
各方向可按风格换 display，中文必须带 Noto Sans SC。版本号/控制台/端口用等宽。

## 气质

调酒师工具，不是酒吧菜单，也不是 Azure/Pterodactyl 换皮。关键词：吧台、装瓶、流动 vs 凝固、可追溯、热接管。
