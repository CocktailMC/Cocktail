# Build Windows MSI (and portable zip) for Cocktail Manager.
# Requires: cargo, npm
# Optional: WiX Toolset v3 (candle.exe + light.exe) for MSI
param(
  [string]$Version = ""
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

if (-not $Version) {
  $m = Select-String -Path "Cargo.toml" -Pattern 'version\s*=\s*"([^"]+)"' | Select-Object -First 1
  $Version = $m.Matches[0].Groups[1].Value
}
Write-Host "==> version=$Version"

Write-Host "==> cargo build --release"
cargo build -p cocktail-control --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "==> admin npm build"
Push-Location admin
$npmOk = $false
if (Test-Path package-lock.json) {
  npm ci
  if ($LASTEXITCODE -eq 0) { $npmOk = $true }
  else { Write-Host "==> npm ci failed (often file lock from vite). Retrying npm install ..." }
}
if (-not $npmOk) {
  npm install
  if ($LASTEXITCODE -ne 0) { Pop-Location; exit $LASTEXITCODE }
}
npm run build
if ($LASTEXITCODE -ne 0) { Pop-Location; exit $LASTEXITCODE }
Pop-Location

$Stage = Join-Path $Root "dist\stage-win"
$Out = Join-Path $Root "dist"
Remove-Item -Recurse -Force $Stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Stage | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $Stage "web") | Out-Null
New-Item -ItemType Directory -Force -Path $Out | Out-Null

Copy-Item "target\release\cocktail-control.exe" (Join-Path $Stage "cocktail-control.exe")
Copy-Item -Recurse -Force "admin\dist\*" (Join-Path $Stage "web")
Copy-Item "packaging\env\cocktail.env" (Join-Path $Stage "cocktail.env.example")
Copy-Item "logo.png" (Join-Path $Stage "logo.png") -ErrorAction SilentlyContinue

@"
Cocktail Manager $Version
========================
1. Set working directory to a data folder (or use Start Menu shortcut).
2. Ensure COCKTAIL_WEB_ROOT points to the web\ folder next to the exe
   (installer sets this system-wide).
3. Run cocktail-control.exe 鈥?open http://127.0.0.1:11011
"@ | Set-Content -Encoding UTF8 (Join-Path $Stage "README.txt")

# Portable zip always
$Zip = Join-Path $Out "cocktail-$Version-windows-x64.zip"
if (Test-Path $Zip) { Remove-Item $Zip }
Compress-Archive -Path (Join-Path $Stage "*") -DestinationPath $Zip -Force
Write-Host "==> zip: $Zip"

# MSI via WiX if available
$candle = Get-Command candle.exe -ErrorAction SilentlyContinue
$light = Get-Command light.exe -ErrorAction SilentlyContinue
$heat = Get-Command heat.exe -ErrorAction SilentlyContinue

if (-not $candle -or -not $light -or -not $heat) {
  Write-Host @"
==> WiX Toolset not found (candle/light/heat). MSI skipped.
    Install WiX v3: https://wixtoolset.org/
    Or: winget install WiXToolset.WiXToolset
    Portable zip is ready: $Zip
"@
  exit 0
}

$WixOut = Join-Path $Out "wix"
New-Item -ItemType Directory -Force -Path $WixOut | Out-Null
$WebHarvest = Join-Path $WixOut "WebComponents.wxs"
$WebWixobj = Join-Path $WixOut "WebComponents.wixobj"
$MainWixobj = Join-Path $WixOut "Cocktail.wixobj"

& heat.exe dir (Join-Path $Stage "web") -cg WebComponents -gg -sfrag -srd -dr WebFolder -var var.WebDir -out $WebHarvest
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

& candle.exe -nologo `
  "-dProductVersion=$Version" `
  "-dStageDir=$Stage" `
  "-dWebDir=$(Join-Path $Stage 'web')" `
  "packaging\windows\Cocktail.wxs" `
  $WebHarvest `
  -o "$WixOut\"
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$Msi = Join-Path $Out "cocktail-$Version-windows-x64.msi"
& light.exe -nologo -ext WixUIExtension `
  (Join-Path $WixOut "Cocktail.wixobj") `
  $WebWixobj `
  -o $Msi
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Write-Host "==> msi: $Msi"
