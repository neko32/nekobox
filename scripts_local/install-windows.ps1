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
#   ├── backend\    (Dockerfile + ソース - Docker ビルド用)
#   ├── frontend\   (Godot プロジェクト)
#   ├── logs\
#   ├── start.ps1   (起動スクリプト)
#   └── stop.ps1    (停止スクリプト)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$ScriptDir   = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = Split-Path -Parent $ScriptDir

function Write-Step { param([string]$Msg) Write-Host "[install] $Msg" -ForegroundColor Cyan }
function Write-Ok   { param([string]$Msg) Write-Host "[install] $Msg" -ForegroundColor Green }
function Write-Warn { param([string]$Msg) Write-Host "[install] $Msg" -ForegroundColor Yellow }
function Write-Fail { param([string]$Msg) Write-Host "[install] $Msg" -ForegroundColor Red }

# ---------------------------------------------------------------------------
# 前提チェック
# ---------------------------------------------------------------------------
Write-Step "前提条件を確認しますまる..."

if (-not $env:NEKOKAN_BIN_DIR) {
    Write-Fail "環境変数 NEKOKAN_BIN_DIR が設定されていないまる。"
    Write-Fail "例: `$env:NEKOKAN_BIN_DIR = 'C:\tools\bin' を設定してから再実行してまる。"
    exit 1
}

$dockerCmd = Get-Command docker -ErrorAction SilentlyContinue
if (-not $dockerCmd) {
    Write-Fail "docker コマンドが見つからないまる。Docker Desktop をインストール・起動してから再実行してまる。"
    exit 1
}

$dockerInfo = docker info 2>&1
if ($LASTEXITCODE -ne 0) {
    Write-Fail "Docker Desktop が起動していないまる。起動してから再実行してまる。"
    exit 1
}

$DefaultGodotExe = "C:\resources\common\Godot_v4.6.1-stable_mono_win64\Godot_v4.6.1-stable_mono_win64\Godot_v4.6.1-stable_mono_win64.exe"
$GodotExe = if ($env:GODOT_PATH) { $env:GODOT_PATH } else { $DefaultGodotExe }

if (Test-Path $GodotExe) {
    Write-Ok "Godot を検出したまる: $GodotExe"
} else {
    Write-Warn "Godot が見つからないまる。フロントエンドの起動スクリプトは生成しますが、実行時に Godot が必要まる。"
}

Write-Ok "前提条件チェック完了まる。"
Write-Host ""

# ---------------------------------------------------------------------------
# ディレクトリ作成
# ---------------------------------------------------------------------------
$InstallDir = Join-Path $env:NEKOKAN_BIN_DIR "nekobox"
Write-Step "インストール先ディレクトリを準備しますまる: $InstallDir"

foreach ($dir in @($InstallDir, "$InstallDir\config", "$InstallDir\logs")) {
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
Copy-Item -Recurse -Force -Path "$ProjectRoot\config\*" -Destination "$InstallDir\config\"
Write-Ok "  config/ をコピーしましたまる。"

Write-Step "バックエンドソースをコピーしますまる (Docker ビルド用)..."
if (Test-Path "$InstallDir\backend") { Remove-Item -Recurse -Force "$InstallDir\backend" }
Copy-Item -Recurse -Force -Path "$ProjectRoot\backend" -Destination $InstallDir
Write-Ok "  backend/ をコピーしましたまる。"

Write-Step "フロントエンドをコピーしますまる..."
if (Test-Path "$InstallDir\frontend") { Remove-Item -Recurse -Force "$InstallDir\frontend" }
Copy-Item -Recurse -Force -Path "$ProjectRoot\frontend" -Destination $InstallDir
Write-Ok "  frontend/ をコピーしましたまる。"

Write-Step "docker-compose.yml を調整してコピーしますまる..."
$dcContent = Get-Content "$ProjectRoot\deploy\docker-compose.yml" -Raw
$dcContent  = $dcContent -replace 'context:\s*\.\./backend', 'context: ./backend'
Set-Content -Path "$InstallDir\docker-compose.yml" -Value $dcContent -Encoding UTF8
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
# 起動・停止スクリプトをテンプレートからコピー
# ---------------------------------------------------------------------------
Write-Step "start.ps1 / stop.ps1 を配置しますまる..."

# stop.ps1 はそのままコピー
Copy-Item -Force -Path "$ScriptDir\stop-template.ps1" -Destination "$InstallDir\stop.ps1"
Write-Ok "  stop.ps1 を配置しましたまる。"

# start.ps1 はデフォルト Godot パスのプレースホルダを置換してコピー
$startContent = Get-Content "$ScriptDir\start-template.ps1" -Raw
$startContent  = $startContent -replace '__DEFAULT_GODOT_EXE__', $GodotExe
Set-Content -Path "$InstallDir\start.ps1" -Value $startContent -Encoding UTF8
Write-Ok "  start.ps1 を配置しましたまる (Godot: $GodotExe)。"
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
Write-Host "  ログ (stdout)  : $InstallDir\logs\backend.log"
Write-Host "  ログ (stderr)  : $InstallDir\logs\backend-error.log"
Write-Host ""
