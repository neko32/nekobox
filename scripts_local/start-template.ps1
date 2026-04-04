# nekobox 起動スクリプト
# このファイルは install-windows.ps1 によってインストール先に配置されます。
#
# 使い方:
#   .\start.ps1           通常起動 (DOS画面抑制、ログはファイルへ)
#   .\start.ps1 -Debug    デバッグ起動 (DOS画面表示)

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
# 環境変数 (未設定の場合のみデフォルト値を適用)
# ---------------------------------------------------------------------------
if (-not $env:NEKOBOX_CFG_PATH)      { $env:NEKOBOX_CFG_PATH      = "$InstallDir\config" }
if (-not $env:NEKOBOX_DB_PATH)       { $env:NEKOBOX_DB_PATH       = "$InstallDir\data" }
if (-not $env:NEKOBOX_BIND_HOST)     { $env:NEKOBOX_BIND_HOST     = "127.0.0.1" }
if (-not $env:NEKOBOX_LMSTUDIO_HOST) { $env:NEKOBOX_LMSTUDIO_HOST = "localhost" }
if (-not $env:NEKOBOX_LMSTUDIO_PORT) { $env:NEKOBOX_LMSTUDIO_PORT = "1234" }
if (-not $env:RUST_LOG)              { $env:RUST_LOG              = "info" }

Write-Info "設定を確認しますまる..."
Write-Host "  InstallDir  : $InstallDir"
Write-Host "  Config Path : $env:NEKOBOX_CFG_PATH"
Write-Host "  LM Studio   : $env:NEKOBOX_LMSTUDIO_HOST:$env:NEKOBOX_LMSTUDIO_PORT"
Write-Host "  RUST_LOG    : $env:RUST_LOG"
Write-Host "  Debug Mode  : $Debug"
Write-Host ""

# ---------------------------------------------------------------------------
# ログディレクトリ確認
# ---------------------------------------------------------------------------
$logDir = "$InstallDir\logs"
if (-not (Test-Path $logDir)) {
    New-Item -ItemType Directory -Path $logDir | Out-Null
}

# ---------------------------------------------------------------------------
# バックエンド起動 (Docker Compose)
# ---------------------------------------------------------------------------
Write-Info "バックエンドを起動しますまる..."

if ($Debug) {
    # デバッグモード: DOS画面を表示して docker compose up を実行
    Write-Info "[DEBUG] コンソールウィンドウを表示して起動しますまる。"
    Start-Process powershell `
        -ArgumentList '-NoExit', '-Command', "Set-Location '$InstallDir'; docker compose up" `
        -WorkingDirectory $InstallDir
} else {
    # 通常モード: DOS画面を抑制し stdout/stderr をログファイルへ出力
    Start-Process docker `
        -ArgumentList 'compose', 'up' `
        -WorkingDirectory $InstallDir `
        -WindowStyle Hidden `
        -RedirectStandardOutput $StdoutLog `
        -RedirectStandardError  $StderrLog

    Write-Ok "バックエンドをバックグラウンドで起動しましたまる。"
    Write-Info "ログ(stdout): $StdoutLog"
    Write-Info "ログ(stderr): $StderrLog"
}

Write-Host ""

# ---------------------------------------------------------------------------
# フロントエンド起動 (Godot)
# ---------------------------------------------------------------------------
if (-not (Test-Path $GodotExe)) {
    Write-Warn "Godot が見つからないまる: $GodotExe"
    Write-Warn "GODOT_PATH 環境変数を設定して再実行するか、手動でフロントエンドを起動してまる。"
    exit 0
}

Write-Info "フロントエンドを起動しますまる..."
Write-Host "  Godot   : $GodotExe"
Write-Host "  Project : $InstallDir\frontend"
Write-Host ""

& $GodotExe --path "$InstallDir\frontend"
