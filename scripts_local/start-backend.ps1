# nekobox バックエンドサーバ起動スクリプト (ローカル開発用)

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = Split-Path -Parent $ScriptDir

# 環境変数設定
$env:NEKOBOX_DB_PATH       = "$ProjectRoot\backend"
$env:NEKOBOX_LMSTUDIO_HOST = "localhost"
$env:NEKOBOX_LMSTUDIO_PORT = "1234"
$env:NEKOBOX_CFG_PATH      = "$ProjectRoot\config"
$env:NEKOBOX_BIND_HOST     = "127.0.0.1"
$env:RUST_LOG              = "info"

Write-Host "[nekobox] バックエンドサーバを起動しますまる..." -ForegroundColor Cyan
Write-Host "  DB Path     : $env:NEKOBOX_DB_PATH"
Write-Host "  LM Studio   : $env:NEKOBOX_LMSTUDIO_HOST`:$env:NEKOBOX_LMSTUDIO_PORT"
Write-Host "  Config Path : $env:NEKOBOX_CFG_PATH"
Write-Host "  Bind        : $env:NEKOBOX_BIND_HOST`:8080"
Write-Host ""

Set-Location "$ProjectRoot\backend"
cargo run
