# 本地一键验证（替代已取消的 GitHub CI）。
# 用法：pwsh -File scripts/verify.ps1
# 需要：Node 22+、Rust MSVC 工具链。

$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

Write-Host '==> 版本一致性' -ForegroundColor Cyan
node scripts/check-version.mjs

Write-Host '==> 前端测试' -ForegroundColor Cyan
npm test

Write-Host '==> 前端 lint' -ForegroundColor Cyan
npm run lint

Write-Host '==> 前端构建' -ForegroundColor Cyan
npm run build

Write-Host '==> cargo fmt' -ForegroundColor Cyan
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check

Write-Host '==> cargo clippy' -ForegroundColor Cyan
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings

Write-Host '==> cargo test' -ForegroundColor Cyan
cargo test --manifest-path src-tauri/Cargo.toml --lib

Write-Host '全部通过。' -ForegroundColor Green
