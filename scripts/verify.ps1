# 本地一键验证（替代已取消的 GitHub CI）。
# 用法：pwsh -File scripts/verify.ps1
# 需要：PowerShell 7+（pwsh）、Node 22.6+、Rust 1.88+ MSVC 工具链。
# Windows PowerShell 5.1 不支持本脚本的调用方式，请安装 PowerShell 7。

# 统一输出编码，避免中文在非 UTF-8 控制台里乱码
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new()

$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

# PowerShell 7 默认不把原生命令的非零退出码转为终止错误。
function Invoke-Check {
    param([string]$Title, [string]$Executable, [string[]]$Arguments)
    Write-Host "==> $Title" -ForegroundColor Cyan
    & $Executable @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Title 失败（退出码 $LASTEXITCODE）"
    }
}

Invoke-Check '版本一致性' 'node' @('scripts/check-version.mjs')

Invoke-Check '前端测试' 'npm' @('test')

Invoke-Check '前端 lint' 'npm' @('run', 'lint')

Invoke-Check '前端 format 检查' 'npx' @('prettier', '--check', '.')

Invoke-Check '前端构建' 'npm' @('run', 'build')

Invoke-Check 'cargo fmt' 'cargo' @('fmt', '--manifest-path', 'src-tauri/Cargo.toml', '--', '--check')

Invoke-Check 'cargo clippy' 'cargo' @('clippy', '--manifest-path', 'src-tauri/Cargo.toml', '--all-targets', '--', '-D', 'warnings')

Invoke-Check 'cargo test' 'cargo' @('test', '--manifest-path', 'src-tauri/Cargo.toml', '--lib')

Write-Host '全部通过。' -ForegroundColor Green
