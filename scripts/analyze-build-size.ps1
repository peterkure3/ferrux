#Requires -Version 5.1
[CmdletBinding()]
param(
    [string]$Repository = (Split-Path $PSScriptRoot -Parent),
    [string]$TargetDirectory,
    [switch]$IncludeGlobalCaches,
    [string]$JsonPath
)
$ErrorActionPreference = 'Stop'
$Repository = (Resolve-Path -LiteralPath $Repository).Path
if (!$TargetDirectory) {
    $TargetDirectory = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Repository 'target' }
}
$TargetDirectory = [IO.Path]::GetFullPath($TargetDirectory)

function Measure-Directory([string]$Path, [string]$Scope) {
    $files = @()
    if (Test-Path -LiteralPath $Path) {
        # Fail on inaccessible files rather than silently undercounting.
        $files = @(Get-ChildItem -LiteralPath $Path -Recurse -File -Force)
    }
    [pscustomobject]@{
        Scope = $Scope; Path = $Path; Bytes = [long](($files | Measure-Object Length -Sum).Sum)
        Files = $files.Count
    }
}
$sourceBytes = [long]0
$sourceFiles = 0
foreach ($item in Get-ChildItem -LiteralPath $Repository -Force) {
    if ($item.Name -in @('.git', 'target') -or $item.FullName -eq $TargetDirectory) { continue }
    if ($item.PSIsContainer) {
        $size = Measure-Directory $item.FullName 'source'
        $sourceBytes += $size.Bytes; $sourceFiles += $size.Files
    } else { $sourceBytes += $item.Length; $sourceFiles++ }
}
$top = @([pscustomobject]@{Scope='source'; Path=$Repository; Bytes=$sourceBytes; Files=$sourceFiles})
$top += Measure-Directory (Join-Path $Repository '.git') 'git'
$top += Measure-Directory $TargetDirectory 'target'
if ($IncludeGlobalCaches) {
    $cargoRoot = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
    foreach ($relative in @('registry/src','registry/cache','registry/index','git')) {
        $top += Measure-Directory (Join-Path $cargoRoot $relative) 'shared Cargo cache (not Ferrux-owned)'
    }
}
$total = [long](($top | Measure-Object Bytes -Sum).Sum)
$details = @()
if (Test-Path -LiteralPath $TargetDirectory) {
    foreach ($child in Get-ChildItem -LiteralPath $TargetDirectory -Directory -Force) {
        $details += Measure-Directory $child.FullName 'target child (overlaps target)'
        foreach ($relative in @('deps','incremental','build')) {
            $details += Measure-Directory (Join-Path $child.FullName $relative) 'profile detail (overlaps parent)'
        }
    }
    $all = @(Get-ChildItem -LiteralPath $TargetDirectory -Recurse -File -Force)
    $extensions = @($all | Group-Object Extension | ForEach-Object {
        [pscustomobject]@{Extension=$_.Name; Bytes=[long](($_.Group | Measure-Object Length -Sum).Sum); Files=$_.Count}
    } | Sort-Object Bytes -Descending)
    $largest = @($all | Sort-Object Length -Descending | Select-Object -First 20 FullName, Length)
} else { $extensions = @(); $largest = @() }
foreach ($row in @($top) + @($details)) {
    $row | Add-Member NoteProperty PercentOfMeasuredTotal $(if ($total) { [math]::Round(100 * $row.Bytes / $total, 4) } else { 0 })
}
$report = [pscustomobject]@{
    TimestampUtc=[DateTime]::UtcNow.ToString('o'); TotalBytes=$total
    Accounting='Logical file lengths; parent/detail and extension rows overlap. Shared caches belong to all projects. NTFS hardlinks may be counted more than once.'
    TopLevel=$top; Details=$details; Extensions=$extensions; LargestFiles=$largest
}
$report.TopLevel | Format-Table Bytes,Files,PercentOfMeasuredTotal,Scope,Path -AutoSize -Wrap
$report.Details | Format-Table Bytes,Files,PercentOfMeasuredTotal,Scope,Path -AutoSize -Wrap
$report.Extensions | Format-Table -AutoSize
$report.LargestFiles | Format-Table Length,FullName -AutoSize -Wrap
if ($JsonPath) { $report | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $JsonPath -Encoding UTF8 }
