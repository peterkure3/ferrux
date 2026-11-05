#Requires -Version 5.1
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$TargetDirectory,
    [ValidateSet('dev','release')][string]$Profile = 'dev',
    [Parameter(Mandatory=$true)][string]$OutputPath,
    [string]$Configuration
)
$ErrorActionPreference = 'Stop'
$arguments = @('build','--workspace','--locked','--offline','--profile',$Profile,'--target-dir',$TargetDirectory)
if ($Configuration) { $arguments += @('--config', $Configuration) }
$timer = [Diagnostics.Stopwatch]::StartNew()
& cargo @arguments
$code = $LASTEXITCODE
$timer.Stop()
if ($code -ne 0) { throw "Cargo exited with $code" }
$folder = if ($Profile -eq 'dev') { 'debug' } else { $Profile }
$files = @(Get-ChildItem -LiteralPath (Join-Path $TargetDirectory $folder) -Recurse -File -Force)
$binaries = @(Get-ChildItem -LiteralPath (Join-Path $TargetDirectory $folder) -Filter '*.exe' -File | Select-Object Name,Length)
[pscustomobject]@{TimestampUtc=[DateTime]::UtcNow.ToString('o'); Profile=$Profile; Seconds=$timer.Elapsed.TotalSeconds; Bytes=[long](($files | Measure-Object Length -Sum).Sum); Binaries=$binaries; Command="cargo $($arguments -join ' ')"} | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $OutputPath -Encoding UTF8
