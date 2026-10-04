# Cocktail 发版脚本

规则在 [`rules.toml`](rules.toml)。版本字符串的定义在 [CocktailMC/docs 的 versioning.md](https://github.com/CocktailMC/docs/blob/main/versioning.md)。

## 自动 DT

推到 `main` 时，[`.github/workflows/dt-release.yml`](../../.github/workflows/dt-release.yml) 会先跑检查，再执行：

```bash
python3 scripts/release/cocktail_release.py plan --mode auto --build-id "$GITHUB_RUN_NUMBER"
```

发布条件：

- 自最近一个规范 tag 之后，有至少 2 个计入的合并 PR。
- 计入：改了控制面、管理端、插件或打包。
- 不计入：改动文件全部是 Markdown、`design/`、`docs/`、`.github/`、`logo.png`、`logo.ai`。
- 不计入：带 `release:skip` 或 `dt:skip` 标签。
- Rust fmt、clippy、`cocktail-control` 测试、四个 WASM 插件、管理端 lint 和 build 都通过。

一次流水线最多发一个 DT。tag 形如 `v26Q4.01.DT.01`，文件名才带 `+B12`。已有 tag 不会被覆盖。

`Cargo.toml` 里的 `26.4.11-DP` 仍是 Cargo 能接受的包版本。产品版本由这个脚本生成，不写回 `Cargo.toml`，因为 `26Q4.01.DT.01` 不是 Cargo semver。

## 人工阶段

DP、AT、BT、RC、GA、HF、LTS 不会因为 PR 数量自动升级。在 Actions 里手动运行 **DT release**，`confirm` 填 `release`。

- `bump=major`：本季度主版本加一，小版本用输入的 `minor`。
- `bump=minor`：沿用该阶段当前主版本，小版本加一。本季度还没有这个阶段时会失败。

## 本地试算

```bash
python3 scripts/release/test_cocktail_release.py
python3 scripts/release/cocktail_release.py validate '26Q4.18.AT-D.03+B1842'
```

用夹具看会不会发版，不访问 GitHub：

```bash
python3 scripts/release/cocktail_release.py plan \
  --mode auto \
  --build-id 12 \
  --now 2026-10-04T00:00:00Z \
  --fixture path/to/prs.json
```

夹具格式：

```json
{
  "since": null,
  "tags": ["2026Q4"],
  "prs": [
    {
      "number": 1,
      "title": "fix control plane",
      "merged_at": "2026-10-04T01:00:00Z",
      "labels": [],
      "files": ["crates/cocktail-control/src/lib.rs"]
    }
  ]
}
```

## 制品

Linux 走 `scripts/package_linux.py`，再打一个 stage 目录的 tar.gz。Windows 走 `scripts/package-windows.ps1` 的 zip。装了 WiX 时脚本仍会顺带打 MSI。文件名由 `rename-dist` 改成：

```text
Cocktail.26Q4.01.DT.01+B12.Linux.x86_64.deb
Cocktail.26Q4.01.DT.01+B12.Linux.x86_64.rpm
Cocktail.26Q4.01.DT.01+B12.Linux.x86_64.tar.gz
Cocktail.26Q4.01.DT.01+B12.Windows.x86_64.zip
```

包内的 deb/rpm 版本不带构建号。MSI 不是门禁：运行器上没有 WiX 时只发布 zip。
