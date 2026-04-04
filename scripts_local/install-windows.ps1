# nekobox Windows install script
#
# Usage:
#   .\install-windows.ps1
#
# Requirements:
#   - Environment variable NEKOKAN_BIN_DIR must be set
#   - Docker Desktop must be running
#   - (Optional) Set GODOT_PATH to the Godot executable path
#
# Install destination: $NEKOKAN_BIN_DIR\nekobox\
#   |- docker-compose.yml
#   |- config\
#   |- backend\    (Dockerfile + source for Docker build)
#   |- frontend\   (Godot project)
#   |- logs\
#   |- start.ps1   (launch script)
#   +- stop.ps1    (stop script)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$ScriptDir   = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = Split-Path -Parent $ScriptDir

function Write-Step { param([string]$Msg) Write-Host "[install] $Msg" -ForegroundColor Cyan }
function Write-Ok   { param([string]$Msg) Write-Host "[install] $Msg" -ForegroundColor Green }
function Write-Warn { param([string]$Msg) Write-Host "[install] $Msg" -ForegroundColor Yellow }
function Write-Fail { param([string]$Msg) Write-Host "[install] $Msg" -ForegroundColor Red }

# ---------------------------------------------------------------------------
# Prerequisite checks
# ---------------------------------------------------------------------------
Write-Step "Checking prerequisites..."

if (-not $env:NEKOKAN_BIN_DIR) {
    Write-Fail "Environment variable NEKOKAN_BIN_DIR is not set."
    Write-Fail "Example: `$env:NEKOKAN_BIN_DIR = 'C:\tools\bin'"
    exit 1
}

$dockerCmd = Get-Command docker -ErrorAction SilentlyContinue
if (-not $dockerCmd) {
    Write-Fail "docker command not found. Please install and start Docker Desktop."
    exit 1
}

$dockerInfo = docker info 2>&1
if ($LASTEXITCODE -ne 0) {
    Write-Fail "Docker Desktop is not running. Please start it and retry."
    exit 1
}

$DefaultGodotExe = "C:\resources\common\Godot_v4.6.1-stable_mono_win64\Godot_v4.6.1-stable_mono_win64\Godot_v4.6.1-stable_mono_win64.exe"
$GodotExe = if ($env:GODOT_PATH) { $env:GODOT_PATH } else { $DefaultGodotExe }

if (Test-Path $GodotExe) {
    Write-Ok "Godot detected: $GodotExe"
} else {
    Write-Warn "Godot not found. Start script will be generated but Godot is required at runtime."
}

Write-Ok "Prerequisite check passed."
Write-Host ""

# ---------------------------------------------------------------------------
# Create directories
# ---------------------------------------------------------------------------
$InstallDir = Join-Path $env:NEKOKAN_BIN_DIR "nekobox"
Write-Step "Preparing install directory: $InstallDir"

foreach ($dir in @($InstallDir, "$InstallDir\config", "$InstallDir\data", "$InstallDir\logs")) {
    if (-not (Test-Path $dir)) {
        New-Item -ItemType Directory -Path $dir | Out-Null
        Write-Ok "  Created: $dir"
    } else {
        Write-Warn "  Already exists: $dir (skipped)"
    }
}
Write-Host ""

# ---------------------------------------------------------------------------
# Copy files
# ---------------------------------------------------------------------------
Write-Step "Copying config files..."
Copy-Item -Recurse -Force -Path "$ProjectRoot\config\*" -Destination "$InstallDir\config\"
Write-Ok "  config/ copied."

Write-Step "Copying backend source (for Docker build)..."
Copy-Item -Recurse -Force -Path "$ProjectRoot\backend" -Destination $InstallDir
Write-Ok "  backend/ copied."

Write-Step "Copying frontend..."
Copy-Item -Recurse -Force -Path "$ProjectRoot\frontend" -Destination $InstallDir
Write-Ok "  frontend/ copied."

Write-Step "Copying docker-compose.yml (adjusting context path)..."
$dcContent = Get-Content "$ProjectRoot\deploy\docker-compose.yml" -Raw -Encoding UTF8
$dcContent  = $dcContent -replace 'context:\s*\.\./backend', 'context: ./backend'
$dcContent  = $dcContent -replace '\.\./config:',           './config:'
$dcContent  = $dcContent -replace '\.\./data:',             './data:'
[System.IO.File]::WriteAllText("$InstallDir\docker-compose.yml", $dcContent, [System.Text.Encoding]::UTF8)
Write-Ok "  docker-compose.yml copied."
Write-Host ""

# ---------------------------------------------------------------------------
# Build Docker image
# ---------------------------------------------------------------------------
Write-Step "Building backend Docker image..."
Push-Location $InstallDir
try {
    docker compose build
    if ($LASTEXITCODE -ne 0) {
        Write-Fail "Docker image build failed."
        exit 1
    }
    Write-Ok "Docker image build succeeded."
} finally {
    Pop-Location
}
Write-Host ""

# ---------------------------------------------------------------------------
# Deploy start/stop scripts from templates
# ---------------------------------------------------------------------------
Write-Step "Deploying start.ps1 and stop.ps1..."

# stop.ps1: copy template as-is
Copy-Item -Force -Path "$ScriptDir\stop-template.ps1" -Destination "$InstallDir\stop.ps1"
Write-Ok "  stop.ps1 deployed."

# start.ps1: replace __DEFAULT_GODOT_EXE__ placeholder
$startContent = Get-Content "$ScriptDir\start-template.ps1" -Raw -Encoding UTF8
$startContent  = $startContent -replace '__DEFAULT_GODOT_EXE__', $GodotExe
[System.IO.File]::WriteAllText("$InstallDir\start.ps1", $startContent, [System.Text.Encoding]::UTF8)
Write-Ok "  start.ps1 deployed (Godot: $GodotExe)."
Write-Host ""

# ---------------------------------------------------------------------------
# Done
# ---------------------------------------------------------------------------
Write-Host "============================================================" -ForegroundColor Green
Write-Host " nekobox installation complete!" -ForegroundColor Green
Write-Host "============================================================" -ForegroundColor Green
Write-Host ""
Write-Host "  Install dir : $InstallDir"
Write-Host ""
Write-Host "  Start       : $InstallDir\start.ps1"
Write-Host "  Start(debug): $InstallDir\start.ps1 -Debug"
Write-Host "  Stop        : $InstallDir\stop.ps1"
Write-Host ""
Write-Host "  Log(stdout) : $InstallDir\logs\backend.log"
Write-Host "  Log(stderr) : $InstallDir\logs\backend-error.log"
Write-Host ""
