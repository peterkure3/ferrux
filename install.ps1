#Requires -Version 5.1
<#
.SYNOPSIS
    Installs Ferrux, a native Windows terminal/workspace multiplexer.
.DESCRIPTION
    Downloads the latest (or a pinned) Ferrux release from GitHub,
    installs it to Program Files, and adds it to the machine PATH.
    Requires administrator privileges; if not already elevated, this
    script relaunches itself with a UAC prompt.
.EXAMPLE
    irm https://raw.githubusercontent.com/peterkure3/ferrux/main/install.ps1 | iex
#>
[CmdletBinding()]
param(
    # A release tag (e.g. "v0.1.0"), or "latest".
    [string]$Version = "latest"
)

$ErrorActionPreference = "Stop"

$Repo = "peterkure3/ferrux"
$InstallDir = Join-Path $env:ProgramFiles "Ferrux"
$AssetName = "ferrux-windows-x64.zip"
$ScriptUrl = "https://raw.githubusercontent.com/$Repo/main/install.ps1"

function Test-Admin {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Invoke-Elevate {
    Write-Host "Ferrux installs to '$InstallDir', which needs administrator privileges."
    Write-Host "Requesting elevation (a UAC prompt will appear)..."

    $inner = "irm $ScriptUrl | iex"
    if ($Version -ne "latest") {
        $inner = "`$Version = '$Version'; $inner"
    }
    $arguments = @("-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", $inner)

    try {
        Start-Process -FilePath "powershell.exe" -ArgumentList $arguments -Verb RunAs | Out-Null
    } catch {
        Write-Error "Elevation was declined or failed. Ferrux was not installed."
        exit 1
    }
}

function Get-ReleaseAsset {
    $uri = if ($Version -eq "latest") {
        "https://api.github.com/repos/$Repo/releases/latest"
    } else {
        "https://api.github.com/repos/$Repo/releases/tags/$Version"
    }

    Write-Host "Looking up release '$Version'..."
    $release = Invoke-RestMethod -Uri $uri -Headers @{ "User-Agent" = "ferrux-installer" }

    $asset = $release.assets | Where-Object { $_.name -eq $AssetName }
    if (-not $asset) {
        throw "Release '$($release.tag_name)' has no '$AssetName' asset."
    }
    return $asset
}

function Invoke-DownloadWithProgress {
    param(
        [string]$Uri,
        [string]$OutFile
    )

    $webClient = New-Object System.Net.WebClient
    $progressEventId = "FerruxDownloadProgress"
    $completedEventId = "FerruxDownloadCompleted"

    Register-ObjectEvent -InputObject $webClient -EventName DownloadProgressChanged -SourceIdentifier $progressEventId -Action {
        Write-Progress -Activity "Downloading Ferrux" `
            -Status "$($EventArgs.ProgressPercentage)% ($([math]::Round($EventArgs.BytesReceived / 1MB, 1))MB / $([math]::Round($EventArgs.TotalBytesToReceive / 1MB, 1))MB)" `
            -PercentComplete $EventArgs.ProgressPercentage
    } | Out-Null

    # Register-ObjectEvent action blocks run in their own scope, not the
    # calling function's — $global: is the only reliable way to get the
    # error back out of the event handler.
    $global:FerruxDownloadError = $null
    Register-ObjectEvent -InputObject $webClient -EventName DownloadFileCompleted -SourceIdentifier $completedEventId -Action {
        if ($EventArgs.Error) {
            $global:FerruxDownloadError = $EventArgs.Error
        }
    } | Out-Null

    try {
        $webClient.DownloadFileAsync([Uri]$Uri, $OutFile)
        while ($webClient.IsBusy) {
            Start-Sleep -Milliseconds 100
        }
    } finally {
        Write-Progress -Activity "Downloading Ferrux" -Completed
        Unregister-Event -SourceIdentifier $progressEventId -ErrorAction SilentlyContinue
        Unregister-Event -SourceIdentifier $completedEventId -ErrorAction SilentlyContinue
        $webClient.Dispose()
    }

    $downloadError = $global:FerruxDownloadError
    Remove-Variable -Name FerruxDownloadError -Scope Global -ErrorAction SilentlyContinue
    if ($downloadError) {
        throw $downloadError
    }
}

function Install-Ferrux {
    $asset = Get-ReleaseAsset
    $tempZip = Join-Path $env:TEMP "ferrux-install-$(Get-Random).zip"

    try {
        Invoke-DownloadWithProgress -Uri $asset.browser_download_url -OutFile $tempZip

        Write-Host "Installing to $InstallDir..."
        New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
        Expand-Archive -Path $tempZip -DestinationPath $InstallDir -Force
    } finally {
        Remove-Item -Path $tempZip -ErrorAction SilentlyContinue
    }

    $machinePath = [Environment]::GetEnvironmentVariable("Path", "Machine")
    if (($machinePath -split ";") -notcontains $InstallDir) {
        Write-Host "Adding $InstallDir to the system PATH..."
        [Environment]::SetEnvironmentVariable("Path", "$machinePath;$InstallDir", "Machine")
    }

    Write-Host ""
    Write-Host "Ferrux installed successfully." -ForegroundColor Green
    Write-Host "Open a new terminal and run: ferrux open"
}

if (-not (Test-Admin)) {
    Invoke-Elevate
    return
}

Install-Ferrux
