#Requires -Version 5.1
[CmdletBinding()]
param([string]$ExperimentDirectory = 'target/performance', [string]$ResultsDirectory = 'docs/performance')
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path $ExperimentDirectory | Out-Null
New-Item -ItemType Directory -Force -Path $ResultsDirectory | Out-Null
# Run serially to avoid CPU/memory contention. Each variant starts empty.
foreach ($variant in @('thin','fat','small')) {
    $config = Join-Path $ExperimentDirectory "$variant.toml"
    $settings = @('[profile.release]', ('lto = "' + $(if ($variant -eq 'fat') { 'fat' } else { 'thin' }) + '"'))
    if ($variant -eq 'small') { $settings += @('opt-level = "s"', 'codegen-units = 1') }
    $settings | Set-Content -LiteralPath $config -Encoding UTF8
    $buildTarget = Join-Path $ExperimentDirectory $variant
    if (Test-Path -LiteralPath (Join-Path $buildTarget 'release')) { throw "Use a fresh experiment directory: $buildTarget" }
    & "$PSScriptRoot/measure-build.ps1" -TargetDirectory $buildTarget -Profile release -Configuration $config -OutputPath (Join-Path $ResultsDirectory "build-$variant.json")
    & cargo run --locked --offline --release --target-dir $buildTarget --config $config -p ferrux-tui --example performance
    if ($LASTEXITCODE -ne 0) { throw "Workload failed: $variant" }
    $executable = Join-Path $buildTarget 'release/examples/performance.exe'
    $samples = @(1..5 | ForEach-Object { & $executable })
    $samples | Set-Content -LiteralPath (Join-Path $ResultsDirectory "runtime-$variant.txt") -Encoding UTF8
}
