# nekobox frontend launcher (local dev)

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$ProjectRoot = Split-Path -Parent $ScriptDir

$DefaultGodotExe = "C:\resources\common\Godot_v4.6.1-stable_mono_win64\Godot_v4.6.1-stable_mono_win64\Godot_v4.6.1-stable_mono_win64.exe"
$GodotExe = if ($env:GODOT_PATH) { $env:GODOT_PATH } else { $DefaultGodotExe }

Write-Host '[nekobox] Building C# assembly...' -ForegroundColor Magenta
Set-Location "$ProjectRoot\frontend"
dotnet build
if ($LASTEXITCODE -ne 0) {
    Write-Host '[nekobox] C# build FAILED. Aborting.' -ForegroundColor Red
    exit 1
}

Write-Host ''
Write-Host '[nekobox] Starting Godot frontend...' -ForegroundColor Magenta
Write-Host "  Godot   : $GodotExe"
Write-Host "  Project : $ProjectRoot\frontend"
Write-Host ''

& $GodotExe --path "$ProjectRoot\frontend"