# Build the xConsole installer as ONE self-contained exe (no loose WebView2Loader.dll).
#
# On the GNU toolchain, webview2-com-sys links WebView2Loader.dll dynamically, so the
# installer exe needs that DLL beside it. This script then builds a tiny self-extracting
# stub that embeds both into a single exe. On the MSVC toolchain the loader is
# static-linked, so the installer is already a single exe and no stub is built.
#
# Usage:  installer\build-single-exe.ps1
$ErrorActionPreference = 'Stop'
$installer = Split-Path -Parent $MyInvocation.MyCommand.Path

Write-Host '[1/2] Building the installer...' -ForegroundColor Cyan
Push-Location $installer
try { cargo build --release } finally { Pop-Location }

$innerExe = Join-Path $installer 'target\release\xConsole-Setup.exe'
$innerDll = Join-Path $installer 'target\release\WebView2Loader.dll'

if (-not (Test-Path -LiteralPath $innerDll)) {
    Write-Host ''
    Write-Host 'WebView2Loader is statically linked (MSVC) - the installer is ALREADY a single exe:' -ForegroundColor Green
    Write-Host "  $innerExe"
    exit 0
}

Write-Host '[2/2] Building the single-exe stub (embeds the installer + WebView2Loader.dll)...' -ForegroundColor Cyan
Push-Location (Join-Path $installer 'stub')
try { cargo build --release } finally { Pop-Location }

$out = Join-Path $installer 'stub\target\release\xConsole-Setup.exe'
Write-Host ''
Write-Host 'Single-file installer (ship THIS one):' -ForegroundColor Green
Write-Host "  $out"
Write-Host "  (Do NOT ship $innerExe on its own - it needs WebView2Loader.dll beside it.)"
