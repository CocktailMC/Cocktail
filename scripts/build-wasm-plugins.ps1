param(
  [switch]$DebugBuild
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

rustup target add wasm32-unknown-unknown | Out-Null

$profile = if ($DebugBuild) { "debug" } else { "release" }
$flag = if ($DebugBuild) { @() } else { @("--release") }

$plugins = @(
  @{ crate = "cocktail-plugin-watchdog"; id = "watchdog"; wasm = "cocktail_plugin_watchdog.wasm" },
  @{ crate = "cocktail-plugin-speclint"; id = "speclint"; wasm = "cocktail_plugin_speclint.wasm" },
  @{ crate = "cocktail-plugin-gameops"; id = "gameops"; wasm = "cocktail_plugin_gameops.wasm" },
  @{ crate = "cocktail-plugin-esplus"; id = "esplus"; wasm = "cocktail_plugin_esplus.wasm" }
)

foreach ($p in $plugins) {
  Write-Host "building $($p.id) ($($p.crate)) for wasm32-unknown-unknown"
  cargo build -p $p.crate --target wasm32-unknown-unknown @flag
  $src = Join-Path $root "target/wasm32-unknown-unknown/$profile/$($p.wasm)"
  if (-not (Test-Path $src)) {
    throw "missing $src"
  }
  $destDir = Join-Path $root "crates/plugins/$($p.id)"
  Copy-Item $src (Join-Path $destDir "plugin.wasm") -Force
  $ext = Join-Path $root "data/extensions/$($p.id)"
  New-Item -ItemType Directory -Force -Path $ext | Out-Null
  Copy-Item (Join-Path $destDir "plugin.json") (Join-Path $ext "plugin.json") -Force
  Copy-Item $src (Join-Path $ext "plugin.wasm") -Force
  Write-Host "  -> $($p.id)/plugin.wasm"
}

Write-Host "WASM plugins ready under data/extensions and crates/plugins"
