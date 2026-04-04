# nekobox shortcut creator
#
# Usage:
#   .\create-shortcut.ps1 -InstallDir "C:\tools\bin\nekobox"
#   .\create-shortcut.ps1 -InstallDir "C:\tools\bin\nekobox" -Desktop
#
# Creates nekobox.lnk in $InstallDir.
# With -Desktop, also copies the shortcut to the current user's Desktop.

param(
    [Parameter(Mandatory)]
    [string]$InstallDir,

    [switch]$Desktop
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Write-Ok   { param([string]$Msg) Write-Host "[shortcut] $Msg" -ForegroundColor Green }
function Write-Warn { param([string]$Msg) Write-Host "[shortcut] $Msg" -ForegroundColor Yellow }
function Write-Fail { param([string]$Msg) Write-Host "[shortcut] $Msg" -ForegroundColor Red }

$startScript  = Join-Path $InstallDir "start.ps1"
$iconPath     = Join-Path $InstallDir "app_icon.ico"
$shortcutPath = Join-Path $InstallDir "nekobox.lnk"

if (-not (Test-Path $startScript)) {
    Write-Fail "start.ps1 not found: $startScript"
    Write-Fail "Run install-windows.ps1 first."
    exit 1
}

if (-not (Test-Path $iconPath)) {
    Write-Warn "app_icon.ico not found: $iconPath"
    Write-Warn "Shortcut will be created without a custom icon."
    $iconLocation = ""
} else {
    $iconLocation = "$iconPath,0"
}

# ---------------------------------------------------------------------------
# Create shortcut in install directory
# ---------------------------------------------------------------------------
$wsh = New-Object -ComObject WScript.Shell
$lnk = $wsh.CreateShortcut($shortcutPath)
$lnk.TargetPath      = "powershell.exe"
$lnk.Arguments       = "-WindowStyle Hidden -ExecutionPolicy Bypass -File `"$startScript`""
$lnk.WorkingDirectory = $InstallDir
$lnk.Description     = "nekobox"
if ($iconLocation) { $lnk.IconLocation = $iconLocation }
$lnk.Save()

Write-Ok "Shortcut created: $shortcutPath"

# ---------------------------------------------------------------------------
# Optionally copy to Desktop
# ---------------------------------------------------------------------------
if ($Desktop) {
    $desktopDir     = [Environment]::GetFolderPath("Desktop")
    $desktopShortcut = Join-Path $desktopDir "nekobox.lnk"
    Copy-Item -Force -Path $shortcutPath -Destination $desktopShortcut
    Write-Ok "Desktop shortcut created: $desktopShortcut"
}
