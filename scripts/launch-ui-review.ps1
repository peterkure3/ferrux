#Requires -Version 5.1
[CmdletBinding()]
param([ValidateRange(20,300)][int]$Columns=160,[ValidateRange(8,100)][int]$Rows=48,[ValidatePattern('^[A-Za-z0-9_-]+$')][string]$Name='ferrux')
$ErrorActionPreference='Stop'
$repository=Split-Path $PSScriptRoot -Parent
$review=Join-Path $repository ('target/ui-review/'+$Name+'-'+[Guid]::NewGuid().ToString('N').Substring(0,8))
$binary=Join-Path $review 'bin'
$sessions=Join-Path $review 'sessions'
New-Item -ItemType Directory -Force -Path $binary,$sessions | Out-Null
foreach ($exe in @('ferrux.exe','ferrux-daemon.exe')) {
    Copy-Item -LiteralPath (Join-Path $repository "target/release/$exe") -Destination $binary -Force
}
$workspace=@'
name = "ferrux"
root = { Split = { direction = "Vertical", ratio_percent = 50, first = { Leaf = 1 }, second = { Leaf = 2 } } }
[[panes]]
id = 1
shell = "powershell.exe"
[[panes]]
id = 2
shell = "cmd.exe"
'@
[IO.File]::WriteAllText((Join-Path $sessions "$Name.toml"),$workspace.Replace('name = "ferrux"', ('name = "'+$Name+'"')),[Text.UTF8Encoding]::new($false))
$endpoint='ferrux-review-'+[Guid]::NewGuid().ToString('N').Substring(0,10)
$endpointPath='\\.\pipe\'+$endpoint
$previousPipe=$env:FERRUX_PIPE_NAME
try {
    $env:FERRUX_PIPE_NAME=$endpointPath
    $daemon=Start-Process -FilePath (Join-Path $binary 'ferrux-daemon.exe') -WorkingDirectory $repository -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $review 'daemon.log') -RedirectStandardError (Join-Path $review 'daemon-error.log')
} finally { $env:FERRUX_PIPE_NAME=$previousPipe }
$launcher=Join-Path $review 'open-review.ps1'
$lines=@(
    ('$env:FERRUX_PIPE_NAME = '''+$endpointPath+''''),
    ('$env:FERRUX_SESSION_DIR = '''+$sessions+''''),
    '$env:FERRUX_COLOR_MODE = ''rgb''',
    ('& '''+(Join-Path $binary 'ferrux.exe')+''' open '''+$Name+'''')
)
$lines | Set-Content -LiteralPath $launcher -Encoding UTF8
$arguments='-w new --size '+$Columns+','+$Rows+' new-tab --title "Ferrux Football Heritage Review" --suppressApplicationTitle --startingDirectory "'+$repository+'" powershell.exe -NoProfile -File "'+$launcher+'"'
Start-Process -FilePath (Join-Path $env:LOCALAPPDATA 'Microsoft/WindowsApps/wt.exe') -ArgumentList $arguments -WindowStyle Normal | Out-Null
[pscustomobject]@{DaemonPid=$daemon.Id;PipeName=$endpoint;Workspace=$Name;SessionDirectory=$sessions;BinaryDirectory=$binary;Columns=$Columns;Rows=$Rows} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $review 'instance.json') -Encoding UTF8
Get-Content -LiteralPath (Join-Path $review 'instance.json')
