# nekobox stop script
# Deployed by install-windows.ps1

$InstallDir = Split-Path -Parent $MyInvocation.MyCommand.Path

Write-Host "[nekobox] Stopping backend..." -ForegroundColor Cyan
Push-Location $InstallDir
try {
    docker compose down
    if ($LASTEXITCODE -eq 0) {
        Write-Host "[nekobox] Backend stopped." -ForegroundColor Green
    } else {
        Write-Host "[nekobox] Error occurred while stopping backend." -ForegroundColor Red
    }
} finally {
    Pop-Location
}
