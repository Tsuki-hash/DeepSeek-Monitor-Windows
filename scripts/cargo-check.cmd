@echo off
cd /d "%~dp0..\src-tauri"
cargo check
exit /b %ERRORLEVEL%
