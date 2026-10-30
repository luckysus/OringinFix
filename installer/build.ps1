# 本地打包：cargo build --release + NSIS
# 用法：.\installer\build.ps1
# 可选：.\installer\build.ps1 -Version 0.1.0

param(
    [string]$Version = ""
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

if (-not $Version) {
    $toml = Get-Content "$Root\Cargo.toml" -Raw
    if ($toml -match 'version\s*=\s*"([^"]+)"') {
        $Version = $Matches[1]
    } else {
        throw "无法从 Cargo.toml 读取 version"
    }
}

$Makensis = @(
    "${env:ProgramFiles(x86)}\NSIS\makensis.exe",
    "${env:ProgramFiles}\NSIS\makensis.exe",
    "${env:LOCALAPPDATA}\Programs\NSIS\makensis.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1

if (-not $Makensis) {
    throw "未找到 NSIS（makensis.exe）。请安装：winget install NSIS.NSIS"
}

Write-Host "==> cargo build --release"
cargo build --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

New-Item -ItemType Directory -Force -Path "$Root\dist" | Out-Null
$out = "$Root\dist\OringinFix-Setup-$Version.exe"
if (Test-Path $out) { Remove-Item $out -Force }

# NSIS Unicode 脚本须为 UTF-8 BOM
$nsi = "$Root\installer\OringinFix.nsi"
$bytes = [System.IO.File]::ReadAllBytes($nsi)
if ($bytes.Length -lt 3 -or $bytes[0] -ne 0xEF -or $bytes[1] -ne 0xBB -or $bytes[2] -ne 0xBF) {
    $text = [System.IO.File]::ReadAllText($nsi)
    [System.IO.File]::WriteAllText($nsi, $text, (New-Object System.Text.UTF8Encoding $true))
}

Write-Host "==> makensis  OringinFix $Version"
& $Makensis "/DVERSION=$Version" $nsi
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

Get-ChildItem "$Root\dist\OringinFix-Setup-*.exe" | ForEach-Object {
    Write-Host "OK  $($_.FullName)  ($([math]::Round($_.Length/1MB, 2)) MB)"
}
