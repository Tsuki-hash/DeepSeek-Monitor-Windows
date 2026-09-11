$ErrorActionPreference = "Stop"

function Resolve-VsDevCmd {
  $paths = @()

  $vswhereRoot = ${env:ProgramFiles(x86)}
  if ($vswhereRoot) {
    $vswhere = Join-Path $vswhereRoot "Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path -LiteralPath $vswhere) {
      $installationPath = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
      if ($installationPath) {
        $paths += (Join-Path $installationPath "Common7\Tools\VsDevCmd.bat")
      }
    }
  }

  foreach ($root in @(${env:ProgramFiles}, ${env:ProgramFiles(x86)})) {
    if (-not $root) {
      continue
    }

    foreach ($edition in @("BuildTools", "Community", "Professional", "Enterprise")) {
      $paths += (Join-Path $root "Microsoft Visual Studio\2022\$edition\Common7\Tools\VsDevCmd.bat")
    }
  }

  foreach ($path in $paths) {
    if (Test-Path -LiteralPath $path) {
      return $path
    }
  }

  throw "Visual Studio Build Tools not found. Install Visual Studio Build Tools 2022 with Desktop development with C++."
}

$project = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot "..")).Path
$root = Split-Path -Parent $project

$env:CODEX_DEV_ROOT = $root
$env:RUSTUP_HOME = Join-Path $root ".rustup"
$env:CARGO_HOME = Join-Path $root ".cargo"
$env:npm_config_cache = Join-Path $root ".npm-cache"
$env:npm_config_prefix = Join-Path $root ".npm-global"
$env:TEMP = Join-Path $root ".tmp"
$env:TMP = Join-Path $root ".tmp"

New-Item -ItemType Directory -Force `
  -Path $env:RUSTUP_HOME, $env:CARGO_HOME, $env:npm_config_cache, $env:npm_config_prefix, $env:TEMP, $project `
  | Out-Null

$cargoBin = Join-Path $env:CARGO_HOME "bin"
if (Test-Path -LiteralPath $cargoBin) {
  $env:PATH = "$cargoBin;$env:PATH"
}

if (Test-Path -LiteralPath $env:npm_config_prefix) {
  $env:PATH = "$env:npm_config_prefix;$env:PATH"
}

Set-Location -LiteralPath $project
