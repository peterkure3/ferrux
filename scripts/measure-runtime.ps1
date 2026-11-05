#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$ExperimentDirectory = 'target/performance',
    [string]$OutputPath = 'docs/performance/runtime-paired.json',
    [string[]]$Variants = @('baseline','optimized','thin','fat','small')
)
$ErrorActionPreference = 'Stop'
$rows = @()
for ($sample=0; $sample -lt 7; $sample++) {
    # Rotate order to reduce systematic warmup/temperature bias.
    for ($offset=0; $offset -lt $variants.Count; $offset++) {
        $variant = $variants[($sample+$offset) % $variants.Count]
        $binary = Join-Path $ExperimentDirectory "$variant/release/examples/performance.exe"
        $output = (& $binary) -join "`n"
        if ($LASTEXITCODE -ne 0) { throw "Workload failed: $variant" }
        $parse = [regex]::Match($output, 'parse_ns=(\d+)').Groups[1].Value
        $render = [regex]::Match($output, 'render_ns=(\d+)').Groups[1].Value
        $ipc = [regex]::Match($output, 'ipc_ns=(\d+)').Groups[1].Value
        if (!$parse -or !$render -or !$ipc) { throw "Unexpected benchmark output: $output" }
        $timer = [Diagnostics.Stopwatch]::StartNew()
        & (Join-Path $ExperimentDirectory "$variant/release/ferrux.exe") --help | Out-Null
        $timer.Stop()
        if ($LASTEXITCODE -ne 0) { throw "CLI startup failed: $variant" }
        $rows += [pscustomobject]@{Variant=$variant;Sample=$sample;ParseNs=[long]$parse;TextConstructionNs=[long]$render;IpcSerializationNs=[long]$ipc;CliHelpMs=$timer.Elapsed.TotalMilliseconds}
    }
}
$rows | ConvertTo-Json | Set-Content -LiteralPath $OutputPath -Encoding UTF8
$rows | Format-Table -AutoSize
