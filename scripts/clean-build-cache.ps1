#Requires -Version 5.1
[CmdletBinding(SupportsShouldProcess=$true, ConfirmImpact='Medium')]
param(
    [ValidateSet('Incremental','Debug','Release','Experiments')][string[]]$Scope = @('Incremental'),
    [switch]$Delete
)
$ErrorActionPreference = 'Stop'
$repository = [IO.Path]::GetFullPath((Split-Path $PSScriptRoot -Parent))
$target = Join-Path $repository 'target'
# Only explicit repository-owned paths are eligible. Never follow junctions.
$relativePaths = @()
foreach ($entry in $Scope) {
    $relativePaths += switch ($entry) {
        'Incremental' { 'debug/incremental'; 'release/incremental' }
        'Debug' { 'debug' }
        'Release' { 'release' }
        'Experiments' { 'performance' }
    }
}
$candidates = @()
foreach ($relative in ($relativePaths | Sort-Object -Unique)) {
    $path = [IO.Path]::GetFullPath((Join-Path $target $relative))
    if (!$path.StartsWith($target + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw "Path escapes target: $path"
    }
    if (!(Test-Path -LiteralPath $path)) { continue }
    $ancestor = Get-Item -LiteralPath $path -Force
    while ($ancestor.FullName.Length -ge $target.Length) {
        if ($ancestor.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "Refusing reparse point: $($ancestor.FullName)" }
        $ancestor = $ancestor.Parent
    }
    $children = @(Get-ChildItem -LiteralPath $path -Recurse -Force)
    if ($children | Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint }) {
        throw "Refusing tree containing reparse points: $path"
    }
    $candidates += [pscustomobject]@{Path=$path;Bytes=[long](($children | Where-Object { !$_.PSIsContainer } | Measure-Object Length -Sum).Sum)}
}
# Avoid double-counting when both Debug and Incremental were selected.
$selected = @($candidates | Where-Object {
    $candidatePath = $_.Path
    !($candidates | Where-Object { $candidatePath.StartsWith($_.Path + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) })
})
$selected | Format-Table -AutoSize
$recoverable = [long](($selected | Measure-Object Bytes -Sum).Sum)
Write-Host "Expected recoverable logical bytes: $recoverable (hardlinks can reduce actual reclaimed space)."
if (!$Delete) { Write-Host 'Preview only. Use -Delete to request removal; stop Cargo and Ferrux first.'; return }
if ($selected.Count -eq 0) { return }
# Explicit text confirmation is required even if -Confirm:$false was supplied.
if (!$WhatIfPreference) {
    if ((Read-Host 'Type DELETE to remove only the listed generated directories') -cne 'DELETE') {
        Write-Host 'Cancelled.'; return
    }
}
foreach ($item in $selected) {
    if ($PSCmdlet.ShouldProcess($item.Path, 'Remove generated Cargo artifacts')) {
        Remove-Item -LiteralPath $item.Path -Recurse -Force -Confirm:$false
    }
}
