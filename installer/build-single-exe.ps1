# Build the xConsole installer as ONE self-contained exe (no loose WebView2Loader.dll).
#
# On the GNU toolchain, webview2-com-sys links WebView2Loader.dll dynamically, so the
# installer exe needs that DLL beside it. This script then builds a tiny self-extracting
# stub that embeds both into a single exe. On the MSVC toolchain the loader is
# static-linked, so the installer is already a single exe and no stub is built.
#
# Code signing (the durable fix for AV false positives) is applied automatically when a
# certificate is configured via environment variables — otherwise it is skipped. See
# installer/ANTIVIRUS.md for the full rationale and how to get a certificate.
#   $env:XCONSOLE_SIGN_PFX       = 'C:\path\to\cert.pfx'   # PFX file, OR...
#   $env:XCONSOLE_SIGN_THUMBPRINT= 'ABCD...'               # ...a cert already in your store
#   $env:XCONSOLE_SIGN_PASSWORD  = '...'                   # PFX password (if using a PFX)
#   $env:XCONSOLE_SIGN_TIMESTAMP = 'http://timestamp.digicert.com'  # optional override
#
# Usage:  installer\build-single-exe.ps1
$ErrorActionPreference = 'Stop'
$installer = Split-Path -Parent $MyInvocation.MyCommand.Path

# --- code signing helpers ---------------------------------------------------------------

function Find-SignTool {
    $st = Get-Command signtool.exe -ErrorAction SilentlyContinue
    if ($st) { return $st.Source }
    $kits = "${env:ProgramFiles(x86)}\Windows Kits\10\bin"
    if (Test-Path $kits) {
        # Newest SDK build, x64 binary.
        $found = Get-ChildItem -Path $kits -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
                 Where-Object { $_.FullName -match '\\x64\\' } |
                 Sort-Object FullName -Descending | Select-Object -First 1
        if ($found) { return $found.FullName }
    }
    return $null
}

function Invoke-Sign([string]$path) {
    $pfx   = $env:XCONSOLE_SIGN_PFX
    $thumb = $env:XCONSOLE_SIGN_THUMBPRINT
    if (-not $pfx -and -not $thumb) {
        Write-Host "  (unsigned: set XCONSOLE_SIGN_PFX + XCONSOLE_SIGN_PASSWORD, or XCONSOLE_SIGN_THUMBPRINT, to sign — see installer/ANTIVIRUS.md)" -ForegroundColor DarkYellow
        return
    }
    $signtool = Find-SignTool
    if (-not $signtool) {
        Write-Warning "signtool.exe not found (install the Windows SDK / 'App Installer') — cannot sign $path"
        return
    }
    $ts = if ($env:XCONSOLE_SIGN_TIMESTAMP) { $env:XCONSOLE_SIGN_TIMESTAMP } else { 'http://timestamp.digicert.com' }
    if ($pfx) {
        & $signtool sign /fd SHA256 /tr $ts /td SHA256 /f $pfx /p $env:XCONSOLE_SIGN_PASSWORD $path
    } else {
        & $signtool sign /fd SHA256 /tr $ts /td SHA256 /sha1 $thumb $path
    }
    if ($LASTEXITCODE -ne 0) { Write-Warning "signing failed for $path" }
    else { Write-Host "  signed: $path" -ForegroundColor Green }
}

# --- build ------------------------------------------------------------------------------

Write-Host '[1/2] Building the installer...' -ForegroundColor Cyan
Push-Location $installer
try { cargo build --release } finally { Pop-Location }

$innerExe = Join-Path $installer 'target\release\xConsole-Setup.exe'
$innerDll = Join-Path $installer 'target\release\WebView2Loader.dll'

if (-not (Test-Path -LiteralPath $innerDll)) {
    Write-Host ''
    Write-Host 'WebView2Loader is statically linked (MSVC) - the installer is ALREADY a single exe:' -ForegroundColor Green
    Write-Host "  $innerExe"
    Invoke-Sign $innerExe
    exit 0
}

Write-Host '[2/2] Building the single-exe stub (embeds the installer + WebView2Loader.dll)...' -ForegroundColor Cyan
# Sign the INNER exe before it is embedded, so the unpacked installer is signed too.
Invoke-Sign $innerExe
Push-Location (Join-Path $installer 'stub')
try { cargo build --release } finally { Pop-Location }

$out = Join-Path $installer 'stub\target\release\xConsole-Setup.exe'
# Sign the final single-file launcher — this is the artifact users actually run.
Invoke-Sign $out

Write-Host ''
Write-Host 'Single-file installer (ship THIS one):' -ForegroundColor Green
Write-Host "  $out"
Write-Host "  (Do NOT ship $innerExe on its own - it needs WebView2Loader.dll beside it.)"
