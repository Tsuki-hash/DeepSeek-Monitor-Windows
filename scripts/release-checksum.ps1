# 为最终安装包生成校验和；不签名、不上传或发布。
param([Parameter(Mandatory = $true)][string]$InstallerPath)
$ErrorActionPreference = 'Stop'
$installer = Get-Item -LiteralPath $InstallerPath
$hash = Get-FileHash -LiteralPath $installer.FullName -Algorithm SHA256
$outputPath = $installer.FullName + '.sha256'
[IO.File]::WriteAllText($outputPath, ($hash.Hash.ToLowerInvariant() + '  ' + $installer.Name + "`n"), [Text.UTF8Encoding]::new($false))
Write-Output $outputPath
