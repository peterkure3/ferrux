#Requires -Version 5.1
$ErrorActionPreference = 'Stop'
$repository = Split-Path $PSScriptRoot -Parent
$updater = Join-Path $repository 'crates/ferrux-cli/src/update.ps1'
$fixture = Join-Path $repository ('target/updater-tests/' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $fixture | Out-Null
$wrapper = Join-Path $fixture 'check.ps1'
@'
param($Updater, $Installed, $Tag, $Asset = 'ferrux-windows-x64.zip', [switch]$Offline)
function Invoke-RestMethod {
    if ($Offline) { throw 'Simulated GitHub connection failure' }
    return [pscustomobject]@{
        tag_name = $Tag
        assets = @([pscustomobject]@{ name = $Asset; browser_download_url = 'https://example.invalid/package.zip' })
    }
}
function Start-Process { throw 'A version check must never request elevation' }
function Invoke-WebRequest { throw 'A version check must never download a package' }
function Get-CimInstance { throw 'A version check must work while panes are running' }
& $Updater -InstallDir (Join-Path $PSScriptRoot 'untouched') -ParentPid $PID -CurrentVersion $Installed -CheckOnly
exit $LASTEXITCODE
'@ | Set-Content -LiteralPath $wrapper -Encoding UTF8

$cases = @(
    @{ Installed = '0.1.2'; Tag = 'v0.1.2'; Exit = 0 },
    @{ Installed = '0.2.0'; Tag = 'v0.1.2'; Exit = 0 },
    @{ Installed = '0.1.2'; Tag = 'v0.1.10'; Exit = 10 },
    @{ Installed = '0.1.2'; Tag = '0.2.0'; Exit = 10 },
    @{ Installed = '0.1.2'; Tag = 'v1.0.0'; Exit = 10 },
    @{ Installed = '0.1.2'; Tag = 'not-a-version'; Exit = 1 },
    @{ Installed = '0.1.2'; Tag = 'v0.2.0'; Asset = 'wrong.zip'; Exit = 1 },
    @{ Installed = '0.1.2'; Tag = 'v0.2.0'; Offline = $true; Exit = 1 }
)
foreach ($case in $cases) {
    $arguments = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $wrapper,
        '-Updater', $updater, '-Installed', $case.Installed, '-Tag', $case.Tag)
    if ($case.Asset) { $arguments += @('-Asset', $case.Asset) }
    if ($case.Offline) { $arguments += '-Offline' }
    # Invoke a separate process to exercise the script's actual exit/finally paths.
    try {
        $ErrorActionPreference = 'Continue' # Expected failures write to stderr.
        & powershell.exe @arguments 2>&1 | ForEach-Object { Write-Host $_ }
        $resultCode = $LASTEXITCODE
    } finally { $ErrorActionPreference = 'Stop' }
    if ($resultCode -ne $case.Exit) {
        throw "Expected exit $($case.Exit), got $resultCode for $($case.Installed) -> $($case.Tag)"
    }
    if (Test-Path -LiteralPath (Join-Path $fixture 'untouched')) { throw 'Version check changed the installation' }
    if (-not (Test-Path -LiteralPath $updater)) { throw 'Version check deleted the updater script' }
}

# Load only function definitions, then test real archive extraction and file
# replacement against temporary installations. No network or elevation is used.
$tokens = $null
$parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile($updater, [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw "Updater syntax errors: $parseErrors" }
foreach ($definition in $ast.FindAll({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] }, $false)) {
    . ([scriptblock]::Create($definition.Extent.Text))
}
$AssetName = 'ferrux-windows-x64.zip'
$package = Join-Path $fixture 'package'
New-Item -ItemType Directory -Path $package | Out-Null
foreach ($name in @('ferrux.exe', 'ferrux-daemon.exe')) {
    [IO.File]::WriteAllText((Join-Path $package $name), 'new-binary-' + $name)
}
$archive = Join-Path $fixture $AssetName
Compress-Archive -Path (Join-Path $package '*') -DestinationPath $archive
function Invoke-WebRequest { param($Uri, $OutFile) Copy-Item -LiteralPath $archive -Destination $OutFile }
function Copy-Item {
    param($LiteralPath, $Destination)
    Microsoft.PowerShell.Management\Copy-Item -LiteralPath $LiteralPath -Destination $Destination
    if ($scenario -eq 'rollback-blocked' -and (Split-Path $Destination -Leaf) -eq 'ferrux-daemon.exe') {
        # Lock the just-installed CLI before making the daemon replacement
        # fail, so even restoring the old CLI is temporarily impossible.
        $script:replacementLock = [IO.File]::Open((Join-Path $InstallDir 'ferrux.exe'), 'Open', 'Read', 'Read')
    }
}
$release = [pscustomobject]@{
    tag_name = 'v0.2.0'
    assets = @([pscustomobject]@{
        name = $AssetName
        browser_download_url = 'https://example.invalid/package.zip'
        digest = 'sha256:' + (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    })
}
foreach ($scenario in @('success', 'bad-digest', 'locked-daemon', 'rollback-blocked', 'missing-binary')) {
    $InstallDir = Join-Path $fixture $scenario
    $TempRoot = Join-Path $fixture ($scenario + '-download')
    New-Item -ItemType Directory -Path $InstallDir | Out-Null
    foreach ($name in @('ferrux.exe', 'ferrux-daemon.exe')) {
        [IO.File]::WriteAllText((Join-Path $InstallDir $name), 'old-binary-' + $name)
    }
    $savedDigest = $release.assets[0].digest
    if ($scenario -eq 'bad-digest') { $release.assets[0].digest = 'sha256:' + ('0' * 64) }
    if ($scenario -eq 'missing-binary') {
        $archive = Join-Path $fixture 'missing.zip'
        Compress-Archive -LiteralPath (Join-Path $package 'ferrux.exe') -DestinationPath $archive
        $release.assets[0].digest = 'sha256:' + (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    $lock = $null
    $script:replacementLock = $null
    if ($scenario -in @('locked-daemon', 'rollback-blocked')) {
        $lock = [IO.File]::Open((Join-Path $InstallDir 'ferrux-daemon.exe'), 'Open', 'Read', 'Read')
    }
    $failed = $false
    try { Invoke-Update -Release $release } catch {
        $failed = $true
        Write-Host "$scenario rejected: $($_.Exception.Message)"
    } finally {
        if ($lock) { $lock.Dispose() }
        if ($script:replacementLock) { $script:replacementLock.Dispose() }
        $release.assets[0].digest = $savedDigest
    }
    if ($failed -ne ($scenario -ne 'success')) { throw "Unexpected result for $scenario" }
    if ($scenario -eq 'rollback-blocked') {
        $backup = @(Get-ChildItem -LiteralPath $InstallDir -Recurse -Force -Filter 'ferrux.exe.previous')
        if ($backup.Count -ne 1 -or [IO.File]::ReadAllText($backup[0].FullName) -ne 'old-binary-ferrux.exe') {
            throw 'Failed rollback must retain the original CLI backup'
        }
        continue
    }
    $prefix = if ($scenario -eq 'success') { 'new-binary-' } else { 'old-binary-' }
    foreach ($name in @('ferrux.exe', 'ferrux-daemon.exe')) {
        if ([IO.File]::ReadAllText((Join-Path $InstallDir $name)) -ne ($prefix + $name)) {
            throw "Installation or rollback failed for $scenario/$name"
        }
    }
}
Write-Host "Updater tests passed (8 version checks and 5 installation scenarios). Fixtures: $fixture"
