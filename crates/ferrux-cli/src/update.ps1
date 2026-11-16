# Embedded by ferrux.exe and executed only for an explicit `ferrux update`.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$InstallDir,
    [Parameter(Mandatory = $true)][int]$ParentPid,
    [Parameter(Mandatory = $true)][string]$CurrentVersion,
    [switch]$CheckOnly
)

$ErrorActionPreference = 'Stop'
$Repo = 'peterkure3/ferrux'
$AssetName = 'ferrux-windows-x64.zip'
$TempRoot = Join-Path $env:TEMP ("ferrux-update-{0}" -f [Guid]::NewGuid().ToString('N'))
$LaunchedElevated = $false

function Test-Admin {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Get-FerruxProcesses {
    $installRoot = [IO.Path]::GetFullPath($InstallDir).TrimEnd('\') + '\'
    Get-CimInstance Win32_Process -Filter "Name = 'ferrux.exe' OR Name = 'ferrux-daemon.exe'" |
        Where-Object {
            $_.ExecutablePath -and
            ([IO.Path]::GetFullPath($_.ExecutablePath)).StartsWith(
                $installRoot, [StringComparison]::OrdinalIgnoreCase)
        }
}

function Get-UpdateRelease {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

    Write-Host 'Checking the latest Ferrux release on GitHub...'
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest" `
        -Headers @{ 'User-Agent' = 'Ferrux-Updater' } -TimeoutSec 30
    $releaseVersionText = $release.tag_name -replace '^[vV]', ''
    try { $releaseVersion = [version]$releaseVersionText } catch {
        throw "GitHub release tag '$($release.tag_name)' is not a supported version number."
    }
    if ($releaseVersion -le [version]$CurrentVersion) {
        Write-Host "Ferrux $CurrentVersion is already at the latest published release ($($release.tag_name))." -ForegroundColor Green
        return $null
    }
    $asset = $release.assets | Where-Object { $_.name -eq $AssetName } | Select-Object -First 1
    if (-not $asset) { throw "Release $($release.tag_name) has no $AssetName package." }
    Write-Host "A newer Ferrux release is available: $CurrentVersion -> $($release.tag_name)."
    return $release
}

function Invoke-Update {
    param([Parameter(Mandatory = $true)]$Release)
    New-Item -ItemType Directory -Path $TempRoot | Out-Null
    $asset = $Release.assets | Where-Object { $_.name -eq $AssetName } | Select-Object -First 1

    $zipPath = Join-Path $TempRoot $AssetName
    Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zipPath -UseBasicParsing -TimeoutSec 300
    if ($asset.digest -and $asset.digest -match '^sha256:(.+)$') {
        $expectedDigest = $Matches[1].ToLowerInvariant()
        $actual = (Get-FileHash -LiteralPath $zipPath -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actual -ne $expectedDigest) { throw 'GitHub package SHA-256 digest did not match.' }
    }

    $extractPath = Join-Path $TempRoot 'package'
    Expand-Archive -LiteralPath $zipPath -DestinationPath $extractPath
    $names = @('ferrux.exe', 'ferrux-daemon.exe')
    foreach ($name in $names) {
        if (-not (Test-Path -LiteralPath (Join-Path $extractPath $name) -PathType Leaf)) {
            throw "The release package is missing $name. No files were installed."
        }
    }

    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    $stage = Join-Path $InstallDir ('.ferrux-update-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $stage | Out-Null
    $backups = @{}
    $installed = @()
    $keepStage = $false
    try {
        foreach ($name in $names) {
            $stagedFile = Join-Path $stage $name
            Copy-Item -LiteralPath (Join-Path $extractPath $name) -Destination $stagedFile
            $destination = Join-Path $InstallDir $name
            if (Test-Path -LiteralPath $destination -PathType Leaf) {
                $backup = Join-Path $stage ($name + '.previous')
                [IO.File]::Replace($stagedFile, $destination, $backup)
                $backups[$destination] = $backup
            } else {
                [IO.File]::Move($stagedFile, $destination)
                $backups[$destination] = $null
            }
            $installed += $destination
        }
    } catch {
        $installError = $_
        $rollbackErrors = @()
        for ($index = $installed.Count - 1; $index -ge 0; $index--) {
            $destination = $installed[$index]
            $backup = $backups[$destination]
            try {
                if ($backup -and (Test-Path -LiteralPath $backup)) {
                    [IO.File]::Replace($backup, $destination, [NullString]::Value)
                } elseif (Test-Path -LiteralPath $destination) {
                    Remove-Item -LiteralPath $destination -Force
                }
            } catch {
                $rollbackErrors += $_.Exception.Message
            }
        }
        if ($rollbackErrors.Count) {
            $keepStage = $true
            throw "Update failed: $($installError.Exception.Message). Rollback failed: $($rollbackErrors -join '; '). Backups retained at $stage"
        }
        throw $installError
    } finally {
        if (-not $keepStage) {
            Remove-Item -LiteralPath $stage -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    Write-Host "Ferrux $($release.tag_name) installed in $InstallDir." -ForegroundColor Green
    Write-Host 'Run ferrux open to start the updated version.'
}

try {
    # Version checks are read-only and must work while panes are running,
    # without elevation, downloads, or a separate interactive window.
    $release = Get-UpdateRelease
    if ($CheckOnly) {
        if ($null -ne $release) { exit 10 }
        exit 0
    }
    if ($null -eq $release) { return }

    if (-not (Test-Admin)) {
        $quotedScript = '"{0}"' -f $PSCommandPath
        $quotedDir = '"{0}"' -f $InstallDir
        $arguments = '-NoProfile -ExecutionPolicy Bypass -File {0} -InstallDir {1} -ParentPid {2} -CurrentVersion {3}' -f `
            $quotedScript, $quotedDir, $ParentPid, $CurrentVersion
        Start-Process -FilePath 'powershell.exe' -ArgumentList $arguments -Verb RunAs | Out-Null
        $LaunchedElevated = $true
        Write-Host 'Requested administrator access. Follow the elevated PowerShell window.'
        return
    }

    # This updater is a separate process; wait until the invoking CLI releases
    # its image file before examining other Ferrux processes.
    try { Wait-Process -Id $ParentPid -ErrorAction SilentlyContinue } catch { }
    Start-Sleep -Milliseconds 300
    $active = @(Get-FerruxProcesses)
    if ($active.Count -gt 0) {
        $running = ($active | ForEach-Object { "{0} (PID {1})" -f $_.Name, $_.ProcessId }) -join ', '
        throw "Close Ferrux's UI and daemon before updating so active terminal sessions are not stopped. Still running: $running"
    }

    Invoke-Update -Release $release
} catch {
    if ($CheckOnly) {
        Write-Error "Ferrux update check failed: $($_.Exception.Message)" -ErrorAction Continue
        exit 1
    }
    Write-Host "Ferrux update failed: $($_.Exception.Message)" -ForegroundColor Red
} finally {
    if (-not $CheckOnly -and -not $LaunchedElevated) {
        Remove-Item -LiteralPath $TempRoot -Recurse -Force -ErrorAction SilentlyContinue
        if (Test-Path -LiteralPath $PSCommandPath) {
            Remove-Item -LiteralPath $PSCommandPath -Force -ErrorAction SilentlyContinue
        }
        Write-Host ''
        Read-Host 'Press Enter to close this window' | Out-Null
    }
}
