# nekobox Windows インストールスクリプト
#
# 使い方:
#   .\install-windows.ps1
#
# 前提条件:
#   - 環境変数 NEKOKAN_BIN_DIR が設定済みであること
#   - Docker Desktop が起動済みであること
#   - (任意) 環境変数 GODOT_PATH に Godot 実行ファイルのパスが設定済みであること
#
# インストール先: $NEKOKAN_BIN_DIR\nekobox\
#   ├── docker-compose.yml
#   ├── config\
#   ├── backend\    (Dockerfile + ソース — Docker ビルド用)
#   ├── frontend\   (Godot プロジェクト)
#   ├── start.ps1   (起動スクリプト)
#   └── stop.ps1    (停止スクリプト)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$ScriptDir   = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = Split-Path -Parent $ScriptDir

# ---------------------------------------------------------------------------
# ヘルパー関数
# ---------------------------------------------------------------------------
function Write-Step  { param([string]$Msg) Write-Host "[install] $Msg" -ForegroundColor Cyan }
function Write-Ok    { param([string]$Msg) Write-Host "[install] $Msg" -ForegroundColor Green }
function Write-Warn  { param([string]$Msg) Write-Host "[install] $Msg" -ForegroundColor Yellow }
function Write-Fail  { param([string]$Msg) Write-Host "[install] $Msg" -ForegroundColor Red }

# ---------------------------------------------------------------------------
# 前提チェック
# ---------------------------------------------------------------------------
Write-Step "前提条件を確認しますまる..."

# NEKOKAN_BIN_DIR チェック
if (-not $env:NEKOKAN_BIN_DIR) {
    Write-Fail "環境変数 NEKOKAN_BIN_DIR が設定されていないまる。"
    Write-Fail "例: `$env:NEKOKAN_BIN_DIR = 'C:\tools\bin' を設定してから再実行してまる。"
    exit 1
}

# Docker チェック
$dockerCmd = Get-Command docker -ErrorAction SilentlyContinue
if (-not $dockerCmd) {
    Write-Fail "docker コマンドが見つからないまる。Docker Desktop をインストール・起動してから再実行してまる。"
    exit 1
}

try {
    docker info 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "docker info が失敗" }
} catch {
    Write-Fail "Docker Desktop が起動していないまる。起動してから再実行してまる。"
    exit 1
}

# Godot チェック (任意)
$DefaultGodotExe = "C:\resources\common\Godot_v4.6.1-stable_mono_win64\Godot_v4.6.1-stable_mono_win64\Godot_v4.6.1-stable_mono_win64.exe"
$GodotExe = if ($env:GODOT_PATH) { $env:GODOT_PATH } else { $DefaultGodotExe }
$GodotAvailable = Test-Path $GodotExe

if ($GodotAvailable) {
    Write-Ok "Godot を検出したまる: $GodotExe"
} else {
    Write-Warn "Godot が見つからないまる (GODOT_PATH 未設定 or パスが無効)。フロントエンドの起動スクリプトは生成しますが、実行時に Godot が必要まる。"
}

Write-Ok "前提条件チェック完了まる。"
Write-Host ""

# ---------------------------------------------------------------------------
# ディレクトリ作成
# ---------------------------------------------------------------------------
$InstallDir = Join-Path $env:NEKOKAN_BIN_DIR "nekobox"
Write-Step "インストール先ディレクトリを準備しますまる: $InstallDir"

$dirsToCreate = @(
    $InstallDir,
    "$InstallDir\config",
    "$InstallDir\logs"
)
foreach ($dir in $dirsToCreate) {
    if (-not (Test-Path $dir)) {
        New-Item -ItemType Directory -Path $dir | Out-Null
        Write-Ok "  作成: $dir"
    } else {
        Write-Warn "  既存: $dir (スキップ)"
    }
}
Write-Host ""

# ---------------------------------------------------------------------------
# ファイルのコピー
# ---------------------------------------------------------------------------
Write-Step "設定ファイルをコピーしますまる..."

# config/ → $InstallDir\config\
$configSrc = Join-Path $ProjectRoot "config"
Copy-Item -Recurse -Force -Path "$configSrc\*" -Destination "$InstallDir\config\"
Write-Ok "  config/ をコピーしましたまる。"

# backend/ → $InstallDir\backend\  (Dockerfile + ソース)
Write-Step "バックエンドソースをコピーしますまる (Docker ビルド用)..."
$backendDst = "$InstallDir\backend"
if (Test-Path $backendDst) {
    Remove-Item -Recurse -Force $backendDst
}
Copy-Item -Recurse -Force -Path "$ProjectRoot\backend" -Destination $InstallDir
Write-Ok "  backend/ をコピーしましたまる。"

# frontend/ → $InstallDir\frontend\
Write-Step "フロントエンドをコピーしますまる..."
$frontendDst = "$InstallDir\frontend"
if (Test-Path $frontendDst) {
    Remove-Item -Recurse -Force $frontendDst
}
Copy-Item -Recurse -Force -Path "$ProjectRoot\frontend" -Destination $InstallDir
Write-Ok "  frontend/ をコピーしましたまる。"

# deploy/docker-compose.yml → $InstallDir\docker-compose.yml
# context パスを ../backend → ./backend に書き換えて配置
Write-Step "docker-compose.yml を調整してコピーしますまる..."
$dcSrc = Get-Content "$ProjectRoot\deploy\docker-compose.yml" -Raw
$dcDst = $dcSrc -replace 'context:\s*\.\./backend', 'context: ./backend'
Set-Content -Path "$InstallDir\docker-compose.yml" -Value $dcDst -Encoding UTF8
Write-Ok "  docker-compose.yml をコピーしましたまる。"

Write-Host ""

# ---------------------------------------------------------------------------
# Docker イメージのビルド
# ---------------------------------------------------------------------------
Write-Step "バックエンドの Docker イメージをビルドしますまる..."
Push-Location $InstallDir
try {
    docker compose build
    if ($LASTEXITCODE -ne 0) {
        Write-Fail "Docker イメージのビルドに失敗したまる。"
        exit 1
    }
    Write-Ok "Docker イメージのビルド完了まる。"
} finally {
    Pop-Location
}
Write-Host ""

# ---------------------------------------------------------------------------
# start.ps1 を生成
# ---------------------------------------------------------------------------
Write-Step "start.ps1 を生成しますまる..."

$startScript = @"
# nekobox 起動スクリプト
# 生成元: install-windows.ps1
#
# オプション:
#   -Debug    バックエンドのコンソールウィンドウを表示する (デバッグ用)

param(
    [switch]`$Debug
)

`$InstallDir  = Split-Path -Parent `$MyInvocation.MyCommand.Path
`$LogFile     = "`$InstallDir\logs\backend.log"
`$GodotExe    = if (`$env:GODOT_PATH) { `$env:GODOT_PATH } else { "$GodotExe" }

function Write-Info { param([string]`$Msg) Write-Host "[nekobox] `$Msg" -ForegroundColor Cyan }
function Write-Ok   { param([string]`$Msg) Write-Host "[nekobox] `$Msg" -ForegroundColor Green }
function Write-Fail { param([string]`$Msg) Write-Host "[nekobox] `$Msg" -ForegroundColor Red }

# ---------------------------------------------------------------------------
# 環境変数
# ---------------------------------------------------------------------------
`$env:NEKOBOX_CFG_PATH      = "`$InstallDir\config"
`$env:NEKOBOX_DB_PATH       = "`$InstallDir\data"
`$env:NEKOBOX_BIND_HOST     = "127.0.0.1"
`$env:NEKOBOX_LMSTUDIO_HOST = if (`$env:NEKOBOX_LMSTUDIO_HOST) { `$env:NEKOBOX_LMSTUDIO_HOST } else { "localhost" }
`$env:NEKOBOX_LMSTUDIO_PORT = if (`$env:NEKOBOX_LMSTUDIO_PORT) { `$env:NEKOBOX_LMSTUDIO_PORT } else { "1234" }
`$env:RUST_LOG              = if (`$env:RUST_LOG)              { `$env:RUST_LOG }              else { "info" }

Write-Info "設定を確認しますまる..."
Write-Host "  InstallDir  : `$InstallDir"
Write-Host "  Config Path : `$env:NEKOBOX_CFG_PATH"
Write-Host "  LM Studio   : `$env:NEKOBOX_LMSTUDIO_HOST:`$env:NEKOBOX_LMSTUDIO_PORT"
Write-Host "  RUST_LOG    : `$env:RUST_LOG"
Write-Host "  Debug Mode  : `$Debug"
Write-Host ""

# ---------------------------------------------------------------------------
# バックエンド起動 (Docker Compose)
# ---------------------------------------------------------------------------
Write-Info "バックエンドを起動しますまる..."

if (`$Debug) {
    # デバッグモード: コンソールウィンドウを表示
    Write-Info "[DEBUG] コンソールウィンドウを表示して起動しますまる。"
    Start-Process powershell `
        -ArgumentList '-NoExit', '-Command', "Set-Location '`$InstallDir'; docker compose up" `
        -WorkingDirectory `$InstallDir
} else {
    # 通常モード: コンソールウィンドウを抑制し、ログをファイルに出力
    `$logDir    = "`$InstallDir\logs"
    `$stdoutLog = "`$logDir\backend.log"
    `$stderrLog = "`$logDir\backend-error.log"
    if (-not (Test-Path `$logDir)) { New-Item -ItemType Directory -Path `$logDir | Out-Null }

    Start-Process docker `
        -ArgumentList "compose", "up" `
        -WorkingDirectory `$InstallDir `
        -WindowStyle Hidden `
        -RedirectStandardOutput `$stdoutLog `
        -RedirectStandardError  `$stderrLog

    Write-Ok "バックエンドをバックグラウンドで起動しましたまる。"
    Write-Info "ログ(stdout): `$stdoutLog"
    Write-Info "ログ(stderr): `$stderrLog"
}

Write-Host ""

# ---------------------------------------------------------------------------
# フロントエンド起動 (Godot)
# ---------------------------------------------------------------------------
if (-not (Test-Path `$GodotExe)) {
    Write-Host "[nekobox] Godot が見つからないまる: `$GodotExe" -ForegroundColor Yellow
    Write-Host "[nekobox] GODOT_PATH 環境変数を設定して再実行するか、手動でフロントエンドを起動してまる。" -ForegroundColor Yellow
    exit 0
}

Write-Info "フロントエンドを起動しますまる..."
Write-Host "  Godot   : `$GodotExe"
Write-Host "  Project : `$InstallDir\frontend"
Write-Host ""

& `$GodotExe --path "`$InstallDir\frontend"
"@

Set-Content -Path "$InstallDir\start.ps1" -Value $startScript -Encoding UTF8
Write-Ok "start.ps1 を生成しましたまる: $InstallDir\start.ps1"
Write-Host ""

# ---------------------------------------------------------------------------
# stop.ps1 を生成
# ---------------------------------------------------------------------------
Write-Step "stop.ps1 を生成しますまる..."

$stopScript = @"
# nekobox 停止スクリプト
# 生成元: install-windows.ps1

`$InstallDir = Split-Path -Parent `$MyInvocation.MyCommand.Path

Write-Host "[nekobox] バックエンドを停止しますまる..." -ForegroundColor Cyan
Push-Location `$InstallDir
try {
    docker compose down
    if (`$LASTEXITCODE -eq 0) {
        Write-Host "[nekobox] バックエンドを停止しましたまる。" -ForegroundColor Green
    } else {
        Write-Host "[nekobox] バックエンド停止中にエラーが発生したまる。" -ForegroundColor Red
    }
} finally {
    Pop-Location
}
"@

Set-Content -Path "$InstallDir\stop.ps1" -Value $stopScript -Encoding UTF8
Write-Ok "stop.ps1 を生成しましたまる: $InstallDir\stop.ps1"
Write-Host ""

# ---------------------------------------------------------------------------
# 完了
# ---------------------------------------------------------------------------
Write-Host "============================================================" -ForegroundColor Green
Write-Host " nekobox のインストールが完了したまる！" -ForegroundColor Green
Write-Host "============================================================" -ForegroundColor Green
Write-Host ""
Write-Host "  インストール先 : $InstallDir"
Write-Host ""
Write-Host "  起動 (通常)    : $InstallDir\start.ps1"
Write-Host "  起動 (デバッグ): $InstallDir\start.ps1 -Debug"
Write-Host "  停止           : $InstallDir\stop.ps1"
Write-Host ""
Write-Host "  ログ           : $InstallDir\logs\backend.log"
Write-Host ""
