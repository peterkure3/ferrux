#Requires -Version 5.1
param([string]$BinaryDirectory = (Join-Path (Split-Path $PSScriptRoot -Parent) 'target/release'))
$ErrorActionPreference = 'Stop'
$repository = Split-Path $PSScriptRoot -Parent
$fixture = Join-Path $repository ('target/installer-tests/' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $fixture | Out-Null
$tokens = $null
$parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile((Join-Path $repository 'install.ps1'), [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw "Installer syntax errors: $parseErrors" }
# Load functions without invoking the real installer or modifying machine PATH.
foreach ($definition in $ast.FindAll({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] }, $false)) {
    . ([scriptblock]::Create($definition.Extent.Text))
}
$originalPath = $env:Path
$InstallDir = Join-Path $fixture 'Ferrux installation'
$Version = 'v0.1.2'
$ScriptUrl = 'https://example.invalid/install.ps1'
$script:elevated = $false
$script:exitCode = 0
$script:decline = $false
$script:waited = $false
$script:installed = $false

function Test-Admin { return $script:elevated }
function Install-Ferrux {
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    foreach ($name in @('ferrux.exe', 'ferrux-daemon.exe')) {
        Copy-Item -LiteralPath (Join-Path $BinaryDirectory $name) -Destination (Join-Path $InstallDir $name) -Force
    }
    $script:installed = $true
}
function Start-Process {
    param($FilePath, $ArgumentList, $Verb, $WindowStyle, [switch]$PassThru)
    if ($script:decline) { throw 'Simulated UAC cancellation' }
    if ($Verb -ne 'RunAs' -or -not $PassThru) { throw 'Installer did not request an observable elevated process' }
    $encodedIndex = [Array]::IndexOf($ArgumentList, '-EncodedCommand')
    if ($encodedIndex -lt 0) { throw 'Installer command is not encoded' }
    $decoded = [Text.Encoding]::Unicode.GetString([Convert]::FromBase64String($ArgumentList[$encodedIndex + 1]))
    if ($decoded -notlike "*-Version '$Version'*") { throw 'Elevation lost the requested release version' }
    $process = [pscustomobject]@{ Handle = 1; ExitCode = $script:exitCode }
    $process | Add-Member -MemberType ScriptMethod -Name WaitForExit -Value {
        $script:waited = $true
        if ($this.ExitCode -eq 0) { Install-Ferrux }
    }
    return $process
}

try {
    # A non-admin shell with stale PATH must wait for the elevated copy,
    # preserve custom entries, and resolve the new CLI immediately afterward.
    $sentinel = Join-Path $fixture 'session-only-tools'
    $env:Path = "$sentinel;$originalPath"
    Invoke-FerruxInstaller
    if (-not $script:waited -or -not $script:installed) { throw 'Installer returned before installation finished' }
    if ((Get-Command ferrux -CommandType Application | Select-Object -First 1).Source -ine (Join-Path $InstallDir 'ferrux.exe')) { throw 'CLI is not immediately available' }
    if (($env:Path -split ';') -notcontains $sentinel) { throw 'Session-only PATH entry was lost' }
    ferrux --help | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Bare ferrux command did not run' }

    # Already elevated installations also refresh the invoking environment.
    $script:elevated = $true
    $env:Path = $originalPath
    Invoke-FerruxInstaller
    Enable-FerruxCommand
    if (@($env:Path -split ';' | Where-Object { $_ -ieq $InstallDir }).Count -ne 1) { throw 'Repeated installation duplicated PATH' }

    # Failed or cancelled UAC cannot be reported as a successful install.
    $script:elevated = $false
    foreach ($scenario in @('failed', 'cancelled')) {
        $env:Path = $originalPath
        $script:exitCode = 1
        $script:decline = $scenario -eq 'cancelled'
        $rejected = $false
        try { Invoke-FerruxInstaller } catch { $rejected = $true }
        if (-not $rejected) { throw "Installer accepted $scenario elevation" }
        if ($env:Path -ne $originalPath) { throw 'Failed installation modified session PATH' }
    }

    # An incomplete archive cannot produce success or add a broken command.
    $InstallDir = Join-Path $fixture 'incomplete'
    New-Item -ItemType Directory -Path $InstallDir | Out-Null
    Copy-Item -LiteralPath (Join-Path $BinaryDirectory 'ferrux-daemon.exe') -Destination $InstallDir
    $rejected = $false
    try { Enable-FerruxCommand } catch { $rejected = $true }
    if (-not $rejected) { throw 'An installation missing ferrux.exe was accepted' }
    if ($env:Path -ne $originalPath) { throw 'Incomplete installation modified session PATH' }
    Write-Host "Installer tests passed: stale PATH, elevation wait, direct admin install, repeat install, failed/cancelled UAC, and missing CLI. Fixtures: $fixture"
} finally {
    $env:Path = $originalPath
}
