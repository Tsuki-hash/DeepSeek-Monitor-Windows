. "$PSScriptRoot\env.ps1"

$installerDir = Join-Path $env:CODEX_DEV_ROOT ".install"
$installer = Join-Path $installerDir "rustup-init.exe"
New-Item -ItemType Directory -Force -Path $installerDir | Out-Null

if (-not (Test-Path -LiteralPath $installer)) {
  Invoke-WebRequest `
    -Uri "https://win.rustup.rs/x86_64" `
    -OutFile $installer `
    -UseBasicParsing
}

& $installer -y --no-modify-path --profile minimal --default-host x86_64-pc-windows-msvc

$cargoBin = Join-Path $env:CARGO_HOME "bin"
if (Test-Path -LiteralPath $cargoBin) {
  $env:PATH = "$cargoBin;$env:PATH"
}

rustc --version
cargo --version
