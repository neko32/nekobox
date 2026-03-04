# nekobox backend + frontend launcher (local dev)

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = Split-Path -Parent $ScriptDir

Write-Host '[nekobox] Building backend first...' -ForegroundColor Yellow
Write-Host ''

Set-Location "$ProjectRoot\backend"
cargo build
if ($LASTEXITCODE -ne 0) {
    Write-Host '[nekobox] Build FAILED. Aborting.' -ForegroundColor Red
    exit 1
}

Write-Host ''
Write-Host '[nekobox] Build OK. Starting backend and frontend...' -ForegroundColor Green
Write-Host ''

Start-Process powershell -ArgumentList '-NoExit', '-File', "`"$ScriptDir\start-backend.ps1`""

& "$ScriptDir\start-frontend.ps1"
