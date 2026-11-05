#Requires -Version 5.1
[CmdletBinding(SupportsShouldProcess)]
param(
    [string]$SourceDirectory = (Join-Path (Split-Path $PSScriptRoot -Parent) 'target/release'),
    [string]$InstallDirectory = (Join-Path $env:ProgramFiles 'Ferrux')
)

$ErrorActionPreference = 'Stop'
$SourceDirectory = (Resolve-Path -LiteralPath $SourceDirectory).Path
$InstallDirectory = [IO.Path]::GetFullPath($InstallDirectory)
$names = @('ferrux.exe', 'ferrux-daemon.exe')

foreach ($name in $names) {
    $source = Join-Path $SourceDirectory $name
    $destination = Join-Path $InstallDirectory $name
    if (!(Test-Path -LiteralPath $source -PathType Leaf)) { throw "Release binary missing: $source" }
    if (!(Test-Path -LiteralPath $destination -PathType Leaf)) { throw "Installed binary missing: $destination" }
}

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if (!$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "Administrator rights are required. Open PowerShell with 'Run as administrator' and run: $PSCommandPath"
}

try {
$running = Get-Process ferrux, ferrux-daemon -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -and $_.Path.StartsWith($InstallDirectory, [StringComparison]::OrdinalIgnoreCase) }
if ($running) {
    throw "Close the installed Ferrux UI before updating its executable: $($running.Path -join ', ')"
}

if (!$PSCmdlet.ShouldProcess($InstallDirectory, 'Replace Ferrux CLI and daemon binaries')) { return }

$staged = @()
try {
    foreach ($name in $names) {
        $source = Join-Path $SourceDirectory $name
        $destination = Join-Path $InstallDirectory $name
        $temporary = Join-Path $InstallDirectory ('.' + $name + '.new')
        Copy-Item -LiteralPath $source -Destination $temporary -Force
        $sourceHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $source).Hash
        $stagedHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $temporary).Hash
        if ($sourceHash -ne $stagedHash) { throw "Staged binary verification failed: $name" }
        $staged += $temporary
    }

    foreach ($name in $names) {
        $destination = Join-Path $InstallDirectory $name
        $temporary = Join-Path $InstallDirectory ('.' + $name + '.new')
        [IO.File]::Replace($temporary, $destination, $null)
        $sourceHash = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $SourceDirectory $name)).Hash
        $installedHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $destination).Hash
        if ($sourceHash -ne $installedHash) { throw "Installed binary verification failed: $name" }
        Get-Item -LiteralPath $destination | Select-Object FullName, Length, LastWriteTime
    }
} finally {
    foreach ($temporary in $staged) {
        Remove-Item -LiteralPath $temporary -Force -ErrorAction SilentlyContinue
    }
}

& (Join-Path $InstallDirectory 'ferrux.exe') --version
} catch {
    $errorLog = Join-Path (Split-Path $PSScriptRoot -Parent) 'target/local-install-error.txt'
    $_ | Out-File -LiteralPath $errorLog -Encoding utf8
    throw
}
