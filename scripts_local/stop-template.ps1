# nekobox 停止スクリプト
# このファイルは install-windows.ps1 によってインストール先に配置されます。

$InstallDir = Split-Path -Parent $MyInvocation.MyCommand.Path

Write-Host "[nekobox] バックエンドを停止しますまる..." -ForegroundColor Cyan
Push-Location $InstallDir
try {
    docker compose down
    if ($LASTEXITCODE -eq 0) {
        Write-Host "[nekobox] バックエンドを停止しましたまる。" -ForegroundColor Green
    } else {
        Write-Host "[nekobox] バックエンド停止中にエラーが発生したまる。" -ForegroundColor Red
    }
} finally {
    Pop-Location
}
