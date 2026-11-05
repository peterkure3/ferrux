#Requires -Version 5.1
$ErrorActionPreference = 'Stop'
$repository = Split-Path $PSScriptRoot -Parent
$fixture = Join-Path $repository ('target/script-tests/' + [Guid]::NewGuid().ToString('N'))
$fixtureScripts = Join-Path $fixture 'scripts'
$incremental = Join-Path $fixture 'target/debug/incremental'
New-Item -ItemType Directory -Force -Path $fixtureScripts, $incremental | Out-Null
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'clean-build-cache.ps1') -Destination $fixtureScripts
Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'analyze-build-size.ps1') -Destination $fixtureScripts
[IO.File]::WriteAllBytes((Join-Path $incremental 'cache.bin'), (New-Object byte[] 128))
[IO.File]::WriteAllText((Join-Path $fixture 'Cargo.lock'), 'sentinel-lock')
[IO.File]::WriteAllText((Join-Path $fixture 'main.rs'), 'sentinel-source')
$reportPath = Join-Path $fixture 'report.json'
& (Join-Path $fixtureScripts 'analyze-build-size.ps1') -JsonPath $reportPath
$report = Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json
if (($report.TopLevel | Where-Object Scope -eq 'target').Bytes -ne 128) { throw 'Incorrect target accounting' }
& (Join-Path $fixtureScripts 'clean-build-cache.ps1') -Scope Debug,Incremental
& (Join-Path $fixtureScripts 'clean-build-cache.ps1') -Scope Debug,Incremental -Delete -WhatIf -Confirm:$false
if (!(Test-Path -LiteralPath (Join-Path $incremental 'cache.bin'))) { throw 'Preview removed an artifact' }
if ([IO.File]::ReadAllText((Join-Path $fixture 'Cargo.lock')) -ne 'sentinel-lock') { throw 'Lockfile changed' }
if ([IO.File]::ReadAllText((Join-Path $fixture 'main.rs')) -ne 'sentinel-source') { throw 'Source changed' }
Write-Host "Storage script tests passed; fixture retained at $fixture"
