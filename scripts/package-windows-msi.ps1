# Build Windows MSI using WiX Toolset v7 (wix.exe).
# WiX v7 dropped candle/light/heat; this script generates web components inline.
param(
  [string]$Version = "",
  [string]$StageDir = ""
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

$wix = "C:\Program Files\WiX Toolset v7.0\bin\wix.exe"
if (-not (Test-Path $wix)) {
  $found = Get-Command wix.exe -ErrorAction SilentlyContinue
  if ($found) { $wix = $found.Source }
}
if (-not $wix -or -not (Test-Path $wix)) {
  Write-Error "wix.exe not found. Install WiX Toolset v7."
  exit 1
}

if (-not $Version) {
  $m = Select-String -Path "Cargo.toml" -Pattern 'version\s*=\s*"([^"]+)"' | Select-Object -First 1
  $Version = $m.Matches[0].Groups[1].Value
}
# MSI package version must be numeric (major.minor.build), strip prerelease labels.
$MsiVersion = ($Version -split '-')[0]

if (-not $StageDir) {
  $StageDir = Join-Path $Root "dist\stage-win"
}
if (-not (Test-Path $StageDir)) {
  Write-Error "Stage dir not found: $StageDir. Run package-windows.ps1 first."
  exit 1
}

$Out = Join-Path $Root "dist"
$WixOut = Join-Path $Out "wix"
New-Item -ItemType Directory -Force -Path $WixOut | Out-Null

$WebDir = Join-Path $StageDir "web"
$webFiles = Get-ChildItem -Path $WebDir -Recurse -File

# Collect unique subdirectories under web
$subDirs = @()
foreach ($f in $webFiles) {
  $rel = $f.FullName.Substring($WebDir.Length + 1)
  $dir = Split-Path $rel -Parent
  if ($dir -and $dir -notin $subDirs) { $subDirs += $dir }
}

# Generate web subdirectory Directory elements (nested properly)
$dirElements = @()
foreach ($d in ($subDirs | Sort-Object)) {
  $parts = $d -split '[\\/]'
  $parentId = "WebFolder"
  for ($i = 0; $i -lt $parts.Length; $i++) {
    $segment = $parts[$i]
    $pathSoFar = ($parts[0..$i]) -join '_'
    $dirId = "WebDir_$pathSoFar"
    if ($i -eq $parts.Length - 1) {
      $dirElements += "            <Directory Id=`"$dirId`" Name=`"$segment`" />"
    }
    $parentId = $dirId
  }
}

# Generate web file Components
$webComponents = @()
$webComponents += '  <Fragment>'
$webComponents += '    <ComponentGroup Id="WebComponents" Directory="WebFolder">'
$idx = 0
foreach ($f in $webFiles) {
  $rel = $f.FullName.Substring($WebDir.Length + 1)
  $dirPart = Split-Path $rel -Parent
  $name = Split-Path $rel -Leaf
  $dirId = if ($dirPart) { "WebDir_$($dirPart -replace '[\\/]','_')" } else { "WebFolder" }
  $id = "WebFile$idx"
  $guid = [guid]::NewGuid().ToString().ToUpper()
  $source = "`$(var.WebDir)\$($rel -replace '\\','\\')"
  $escapedSource = $source -replace '\\', '\\'
  $webComponents += "      <Component Id=`"$id`" Guid=`"$guid`" Directory=`"$dirId`">"
  $webComponents += "        <File Id=`"$id`" Source=`"$source`" Name=`"$name`" KeyPath=`"yes`" />"
  $webComponents += "      </Component>"
  $idx++
}
$webComponents += '    </ComponentGroup>'
$webComponents += '  </Fragment>'

# Build complete WiX v4 wxs
$wxs = @"
<Wix xmlns="http://wixtoolset.org/schemas/v4/wxs">
  <Package Name="Cocktail Manager" Language="1033" Version="`$(var.ProductVersion)" Manufacturer="Cocktail" UpgradeCode="A1B2C3D4-E5F6-7890-ABCD-EF1234567890">
    <SummaryInformation Description="Cocktail Manager - Minecraft server control plane" Manufacturer="Cocktail" />
    <MajorUpgrade DowngradeErrorMessage="A newer version of Cocktail Manager is already installed." />
    <MediaTemplate EmbedCab="yes" />

    <Feature Id="ProductFeature" Title="Cocktail Manager" Level="1">
      <ComponentGroupRef Id="ProductComponents" />
      <ComponentGroupRef Id="WebComponents" />
      <ComponentRef Id="ApplicationShortcut" />
    </Feature>

    <StandardDirectory Id="ProgramFiles64Folder">
      <Directory Id="INSTALLFOLDER" Name="Cocktail">
        <Directory Id="WebFolder" Name="web">
$($dirElements -join "`n")
        </Directory>
      </Directory>
    </StandardDirectory>
    <StandardDirectory Id="ProgramMenuFolder">
      <Directory Id="ApplicationProgramsFolder" Name="Cocktail Manager" />
    </StandardDirectory>
    <StandardDirectory Id="CommonAppDataFolder">
      <Directory Id="CocktailDataRoot" Name="Cocktail" />
    </StandardDirectory>
  </Package>

  <Fragment>
    <ComponentGroup Id="ProductComponents" Directory="INSTALLFOLDER">
      <Component Id="MainExecutable" Guid="B2C3D4E5-F6A7-8901-BCDE-F12345678901">
        <File Id="CocktailExe" Source="`$(var.StageDir)\cocktail-control.exe" KeyPath="yes" Checksum="yes" />
        <File Id="CocktailStartCmd" Source="`$(var.StageDir)\Start-Cocktail.cmd" Name="Start-Cocktail.cmd" />
        <Environment Id="CocktailWebRoot" Name="COCKTAIL_WEB_ROOT" Value="[INSTALLFOLDER]web" Permanent="no" Action="set" System="yes" />
        <Environment Id="CocktailBind" Name="COCKTAIL_BIND" Value="127.0.0.1:11011" Permanent="no" Action="set" System="yes" />
      </Component>
      <Component Id="CocktailAgent" Guid="A9B8C7D6-E5F4-3210-BA98-76543210FEDC">
        <File Id="CocktailAgentExe" Source="`$(var.StageDir)\cocktail-agent.exe" KeyPath="yes" Checksum="yes" />
      </Component>
      <Component Id="DataFolder" Guid="F1E2D3C4-B5A6-9780-1234-567890ABCDEF" Directory="CocktailDataRoot">
        <CreateFolder />
        <RegistryValue Root="HKLM" Key="Software\Cocktail\Manager" Name="data" Type="string" Value="[CocktailDataRoot]" KeyPath="yes" />
      </Component>
      <Component Id="EnvExample" Guid="C3D4E5F6-A7B8-9012-CDEF-123456789012">
        <File Id="CocktailEnv" Source="`$(var.StageDir)\cocktail.env.example" Name="cocktail.env.example" KeyPath="yes" />
      </Component>
    </ComponentGroup>

    <DirectoryRef Id="ApplicationProgramsFolder">
      <Component Id="ApplicationShortcut" Guid="E5F6A7B8-C9D0-1234-EF01-345678901234">
        <Shortcut Id="CocktailStartMenu" Name="Cocktail Manager" Description="Start Cocktail control plane" Target="[INSTALLFOLDER]Start-Cocktail.cmd" WorkingDirectory="INSTALLFOLDER" />
        <RemoveFolder Id="CleanMenu" On="uninstall" />
        <RegistryValue Root="HKCU" Key="Software\Cocktail\Manager" Name="installed" Type="integer" Value="1" KeyPath="yes" />
      </Component>
    </DirectoryRef>
  </Fragment>

$($webComponents -join "`n")
</Wix>
"@

$WxsPath = Join-Path $WixOut "Cocktail-v7.wxs"
$utf8 = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllText($WxsPath, $wxs, $utf8)
Write-Host "==> generated $WxsPath ($idx web files)"

$Msi = Join-Path $Out "cocktail-$Version-windows-x64.msi"
if (Test-Path $Msi) { Remove-Item $Msi -Force }

Write-Host "==> wix build"
& $wix build $WxsPath `
  -d ProductVersion="$MsiVersion" `
  -d StageDir="$StageDir" `
  -d WebDir="$WebDir" `
  -arch x64 `
  -o $Msi

if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Write-Host "==> msi: $Msi"
Get-Item $Msi | Select-Object Name, @{N='SizeMB';E={[math]::Round($_.Length/1MB,2)}}
