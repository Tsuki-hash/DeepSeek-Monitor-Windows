. "$PSScriptRoot\env.ps1"

$vsDevCmd = Resolve-VsDevCmd

$cmd = @"
call "$vsDevCmd" -arch=x64 -host_arch=x64 >nul
cd /d "$PWD\src-tauri"
cargo check
"@

$cmdFile = Join-Path $env:TEMP "deepseek-monitor-windows-cargo-check.cmd"
Set-Content -LiteralPath $cmdFile -Value $cmd -Encoding ASCII
cmd /c $cmdFile
