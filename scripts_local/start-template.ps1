# nekobox launch script
# Deployed by install-windows.ps1
#
# Usage:
#   .\start.ps1           Normal mode  (console window suppressed, log to file)
#   .\start.ps1 -Debug    Debug mode   (console window shown)

param(
    [switch]$Debug
)

$InstallDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$StdoutLog  = "$InstallDir\logs\backend.log"
$StderrLog  = "$InstallDir\logs\backend-error.log"
$GodotExe   = if ($env:GODOT_PATH) { $env:GODOT_PATH } else { '__DEFAULT_GODOT_EXE__' }

function Write-Info { param([string]$Msg) Write-Host "[nekobox] $Msg" -ForegroundColor Cyan }
function Write-Ok   { param([string]$Msg) Write-Host "[nekobox] $Msg" -ForegroundColor Green }
function Write-Warn { param([string]$Msg) Write-Host "[nekobox] $Msg" -ForegroundColor Yellow }

# ---------------------------------------------------------------------------
# Environment variables (apply defaults only when not already set)
# ---------------------------------------------------------------------------
if (-not $env:NEKOBOX_CFG_PATH)      { $env:NEKOBOX_CFG_PATH      = "$InstallDir\config" }
if (-not $env:NEKOBOX_DB_PATH)       { $env:NEKOBOX_DB_PATH       = "$InstallDir\data" }
if (-not $env:NEKOBOX_BIND_HOST)     { $env:NEKOBOX_BIND_HOST     = "127.0.0.1" }
if (-not $env:NEKOBOX_LMSTUDIO_HOST) { $env:NEKOBOX_LMSTUDIO_HOST = "localhost" }
if (-not $env:NEKOBOX_LMSTUDIO_PORT) { $env:NEKOBOX_LMSTUDIO_PORT = "1234" }
if (-not $env:RUST_LOG)              { $env:RUST_LOG              = "info" }

Write-Info "Configuration:"
Write-Host "  InstallDir  : $InstallDir"
Write-Host "  Config Path : $env:NEKOBOX_CFG_PATH"
Write-Host "  LM Studio   : $env:NEKOBOX_LMSTUDIO_HOST:$env:NEKOBOX_LMSTUDIO_PORT"
Write-Host "  RUST_LOG    : $env:RUST_LOG"
Write-Host "  Debug Mode  : $Debug"
Write-Host ""

# ---------------------------------------------------------------------------
# Ensure log directory exists
# ---------------------------------------------------------------------------
$logDir = "$InstallDir\logs"
if (-not (Test-Path $logDir)) {
    New-Item -ItemType Directory -Path $logDir | Out-Null
}

# ---------------------------------------------------------------------------
# Start backend (Docker Compose)
# ---------------------------------------------------------------------------
Write-Info "Starting backend..."

if ($Debug) {
    # Debug mode: show console window
    Write-Info "[DEBUG] Launching with visible console window."
    Start-Process powershell `
        -ArgumentList '-NoExit', '-Command', "Set-Location '$InstallDir'; docker compose up" `
        -WorkingDirectory $InstallDir
} else {
    # Normal mode: suppress console window, redirect output to log files
    Start-Process docker `
        -ArgumentList 'compose', 'up' `
        -WorkingDirectory $InstallDir `
        -WindowStyle Hidden `
        -RedirectStandardOutput $StdoutLog `
        -RedirectStandardError  $StderrLog

    Write-Ok "Backend started in background."
    Write-Info "Log(stdout): $StdoutLog"
    Write-Info "Log(stderr): $StderrLog"
}

Write-Host ""

# ---------------------------------------------------------------------------
# Start frontend (Godot)
# ---------------------------------------------------------------------------
if (-not (Test-Path $GodotExe)) {
    Write-Warn "Godot not found: $GodotExe"
    Write-Warn "Set GODOT_PATH and retry, or launch the frontend manually."
    exit 0
}

Write-Info "Starting frontend..."
Write-Host "  Godot   : $GodotExe"
Write-Host "  Project : $InstallDir\frontend"
Write-Host ""

& $GodotExe --path "$InstallDir\frontend"
