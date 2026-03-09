# nekobox summarize ツール起動スクリプト (ローカル開発用)
# session テーブルから session_summary をリフレッシュ生成する

param(
    [switch]$UseLocalEnvvar
)

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = Split-Path -Parent $ScriptDir

# 環境変数設定 (-UseLocalEnvvar 指定時のみスクリプト内の値を使用)
if ($UseLocalEnvvar) {
    $env:NEKOBOX_DB_PATH        = "$ProjectRoot\backend"
    $env:NEKOBOX_LMSTUDIO_HOST  = "localhost"
    $env:NEKOBOX_LMSTUDIO_PORT  = "1234"
    $env:NEKOBOX_MODEL_ID       = "llama-3-elyza-jp-8b"
    $env:NEKOEXPERT_PATH        = "$env:NEKOEXPERT_PATH"
    $env:RUST_LOG               = "info"
    Write-Host "[summarize] スクリプト内の環境変数を使用します" -ForegroundColor Yellow
} else {
    Write-Host "[summarize] OS/シェルの環境変数を使用します" -ForegroundColor Yellow
}

# 必須変数チェック
$missing = @()
foreach ($var in @("NEKOBOX_DB_PATH", "NEKOBOX_LMSTUDIO_HOST", "NEKOBOX_LMSTUDIO_PORT", "NEKOEXPERT_PATH")) {
    if (-not (Get-Item "env:$var" -ErrorAction SilentlyContinue)) {
        $missing += $var
    }
}
if ($missing.Count -gt 0) {
    Write-Host "[summarize] 以下の環境変数が未設定です:" -ForegroundColor Red
    $missing | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
    Write-Host "[summarize] -UseLocalEnvvar オプションを使うか、環境変数を設定してから再実行してください" -ForegroundColor Red
    exit 1
}

Write-Host "[summarize] 設定:" -ForegroundColor Cyan
Write-Host "  DB Path       : $env:NEKOBOX_DB_PATH"
Write-Host "  LM Studio     : $env:NEKOBOX_LMSTUDIO_HOST`:$env:NEKOBOX_LMSTUDIO_PORT"
Write-Host "  Model ID      : $(if ($env:NEKOBOX_MODEL_ID) { $env:NEKOBOX_MODEL_ID } else { '(LM Studio ロード中のモデルを使用)' })"
Write-Host "  Expert Path   : $env:NEKOEXPERT_PATH"
Write-Host ""

# ビルド
Write-Host "[summarize] ビルド中..." -ForegroundColor Yellow
Set-Location "$ProjectRoot\backend"
cargo build --bin summarize
if ($LASTEXITCODE -ne 0) {
    Write-Host "[summarize] ビルド失敗" -ForegroundColor Red
    exit 1
}

Write-Host ""
Write-Host "[summarize] サマリ生成を開始します..." -ForegroundColor Green
Write-Host ""

# 実行
& "$ProjectRoot\backend\target\debug\summarize.exe"
$exitCode = $LASTEXITCODE

if ($exitCode -eq 0) {
    Write-Host ""
    Write-Host "[summarize] 完了" -ForegroundColor Green
} else {
    Write-Host ""
    Write-Host "[summarize] エラーで終了しました (exit code: $exitCode)" -ForegroundColor Red
}

exit $exitCode

