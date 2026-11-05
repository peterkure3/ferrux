#Requires -Version 5.1
[CmdletBinding()]
param([string]$BinaryDirectory = 'target/release', [string]$OutputDirectory = 'target/dist')
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$executables = @('ferrux.exe','ferrux-daemon.exe') | ForEach-Object {
    $path = Join-Path $BinaryDirectory $_
    if (!(Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing release executable: $path" }
    $path
}
Compress-Archive -LiteralPath $executables -DestinationPath (Join-Path $OutputDirectory 'ferrux-windows-x64.zip') -Force
$symbols = @(Get-ChildItem -LiteralPath $BinaryDirectory -Filter '*.pdb' -File | Where-Object { $_.BaseName -in @('ferrux','ferrux_daemon','ferrux-daemon') })
if ($symbols.Count) {
    Compress-Archive -LiteralPath $symbols.FullName -DestinationPath (Join-Path $OutputDirectory 'ferrux-windows-x64-symbols.zip') -Force
}
