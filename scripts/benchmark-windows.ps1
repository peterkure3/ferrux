#Requires -Version 5.1
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$BinaryDirectory,
    [Parameter(Mandatory=$true)][string]$OutputPath,
    [string]$PipeName = 'ferrux-daemon'
)
$ErrorActionPreference = 'Stop'
$BinaryDirectory = (Resolve-Path -LiteralPath $BinaryDirectory).Path
$results = @()
function Connect-Ferrux {
    $pipe = [IO.Pipes.NamedPipeClientStream]::new('.', $PipeName, [IO.Pipes.PipeDirection]::InOut, [IO.Pipes.PipeOptions]::Asynchronous)
    try { $pipe.Connect(200); return $pipe } catch { $pipe.Dispose(); throw }
}
function Read-Exact($Pipe, [int]$Count) {
    $buffer = New-Object byte[] $Count
    $offset = 0
    while ($offset -lt $Count) {
        $task = $Pipe.ReadAsync($buffer, $offset, $Count - $offset)
        if (!$task.Wait(10000)) { throw 'Named pipe read timed out' }
        if ($task.Result -eq 0) { throw 'Unexpected EOF' }
        $offset += $task.Result
    }
    return ,$buffer
}
function Request-Ferrux($Request) {
    $pipe = Connect-Ferrux
    try {
        $json = ConvertTo-Json -InputObject $Request -Compress -Depth 5
        $body = [Text.Encoding]::UTF8.GetBytes($json)
        $header = [BitConverter]::GetBytes([uint32]$body.Length)
        $pipe.Write($header,0,4); $pipe.Write($body,0,$body.Length)
        $length = [BitConverter]::ToUInt32((Read-Exact $pipe 4),0)
        $response = [Text.Encoding]::UTF8.GetString((Read-Exact $pipe $length)) | ConvertFrom-Json
        if ($response.Error) { throw $response.Error.message }
        return $response
    } finally { $pipe.Dispose() }
}
function Attach-Ferrux([long]$Id) {
    $pipe = Connect-Ferrux
    try {
        $body = [Text.Encoding]::UTF8.GetBytes((ConvertTo-Json -InputObject @{AttachPane=@{id=$Id}} -Compress))
        $header = [BitConverter]::GetBytes([uint32]$body.Length)
        $pipe.Write($header,0,4); $pipe.Write($body,0,$body.Length)
        $length = [BitConverter]::ToUInt32((Read-Exact $pipe 4),0)
        $response = [Text.Encoding]::UTF8.GetString((Read-Exact $pipe $length)) | ConvertFrom-Json
        if ($response -ne 'Ok') { throw "Attach failed: $response" }
        return $pipe
    } catch { $pipe.Dispose(); throw }
}
function Read-Until($Pipe, [string]$Marker) {
    $buffer = New-Object byte[] 16384
    $characters = New-Object char[] 16384
    $decoder = [Text.Encoding]::UTF8.GetDecoder()
    $text = [Text.StringBuilder]::new()
    $clock = [Diagnostics.Stopwatch]::StartNew()
    while ($clock.Elapsed.TotalSeconds -lt 20) {
        $task = $Pipe.ReadAsync($buffer,0,$buffer.Length)
        if (!$task.Wait(20000)) { throw "Output timed out: $Marker" }
        if ($task.Result -eq 0) { throw 'Attach closed before output marker' }
        $count = $decoder.GetChars($buffer,0,$task.Result,$characters,0,$false)
        $null = $text.Append($characters,0,$count)
        # ConPTY can insert cursor-control sequences between output glyphs.
        $plain = $text.ToString() -replace '\x1B\[[0-?]*[ -/]*[@-~]', ''
        if ($plain.Contains($Marker)) { return $clock.Elapsed.TotalMilliseconds }
    }
    throw "Missing output marker: $Marker"
}
try {
    $existing = Connect-Ferrux
    $existing.Dispose()
    throw 'A Ferrux daemon is already running; benchmark requires an isolated daemon. Detach/stop it manually first.'
} catch [TimeoutException] { }
$work = Join-Path $BinaryDirectory 'benchmark-working-directory'
New-Item -ItemType Directory -Force -Path $work | Out-Null
$timer = [Diagnostics.Stopwatch]::StartNew()
$previousFerruxPipe = $env:FERRUX_PIPE_NAME
$env:FERRUX_PIPE_NAME = '\\.\pipe\' + $PipeName
$daemon = Start-Process -FilePath (Join-Path $BinaryDirectory 'ferrux-daemon.exe') -WorkingDirectory $work -WindowStyle Hidden -PassThru
$ids = @()
try {
    for ($attempt=0; $attempt -lt 50; $attempt++) {
        try { $null = Request-Ferrux 'ListPanes'; break } catch { Start-Sleep -Milliseconds 100 }
    }
    $timer.Stop()
    $results += [pscustomobject]@{Metric='daemon_ready_ms'; Value=$timer.Elapsed.TotalMilliseconds}
    foreach ($count in @(0,1,4,8)) {
        while ($ids.Count -lt $count) {
            $timer.Restart()
            $response = Request-Ferrux @{SpawnPane=@{shell='powershell.exe';cols=120;rows=40;cwd=$work}}
            $ids += [long]$response.PaneSpawned.id
            $results += [pscustomobject]@{Metric='pane_spawn_ms';Value=$timer.Elapsed.TotalMilliseconds}
        }
        Start-Sleep -Seconds 2
        $daemon.Refresh()
        $cpuStart = $daemon.TotalProcessorTime.TotalSeconds
        Start-Sleep -Seconds 3
        $daemon.Refresh()
        $results += [pscustomobject]@{Metric="panes_${count}_daemon_private_bytes";Value=$daemon.PrivateMemorySize64}
        $results += [pscustomobject]@{Metric="panes_${count}_daemon_handles";Value=$daemon.HandleCount}
        $results += [pscustomobject]@{Metric="panes_${count}_idle_cpu_core_percent";Value=(100*($daemon.TotalProcessorTime.TotalSeconds-$cpuStart)/3)}
        $timer.Restart()
        for ($i=0;$i -lt 100;$i++) { $null = Request-Ferrux 'ListPanes' }
        $results += [pscustomobject]@{Metric="panes_${count}_list_mean_ms";Value=($timer.Elapsed.TotalMilliseconds/100)}
    }
    foreach ($id in $ids) {
        $null = Request-Ferrux @{ResizePane=@{id=$id;cols=80;rows=24}}
        $null = Request-Ferrux @{ResizePane=@{id=$id;cols=120;rows=40}}
    }
    $results += [pscustomobject]@{Metric='resized_panes';Value=$ids.Count}
    $streams = @()
    try {
        foreach ($id in $ids) {
            $pipe = Attach-Ferrux $id
            $streams += $pipe
            # Build the marker in the shell so input echo cannot satisfy it.
            $command = [Text.Encoding]::UTF8.GetBytes("1..2000 | ForEach-Object { Write-Output ('output line ' + `$_) }; Write-Output ('FERRUX' + '_DONE_$id')`r")
            $pipe.Write($command,0,$command.Length)
        }
        for ($index=0;$index -lt $streams.Count;$index++) {
            $latency = Read-Until $streams[$index] "FERRUX_DONE_$($ids[$index])"
            $results += [pscustomobject]@{Metric='output_2000_lines_drain_ms';Value=$latency}
        }
    } finally { foreach ($pipe in $streams) { $pipe.Dispose() } }
    $response = Request-Ferrux 'ListPanes'
    if (@($response.PaneList.panes).Count -ne $ids.Count) { throw 'Detach terminated a daemon-owned pane' }
    $results += [pscustomobject]@{Metric='detached_live_panes';Value=$ids.Count}
    $pipe = Attach-Ferrux $ids[0]
    try {
        $command = [Text.Encoding]::UTF8.GetBytes("Write-Output ('RE' + 'ATTACHED')`r")
        $pipe.Write($command,0,$command.Length)
        $latency = Read-Until $pipe 'REATTACHED'
        $results += [pscustomobject]@{Metric='reattach_response_ms';Value=$latency}
    } finally { $pipe.Dispose() }
    $response = Request-Ferrux @{SpawnPane=@{shell='cmd.exe';cols=80;rows=24;cwd=$work}}
    $cmdId = [long]$response.PaneSpawned.id
    $ids += $cmdId
    $pipe = Attach-Ferrux $cmdId
    try {
        $marker = [string][char]0x754C + '_CMD_OK'
        $command = [Text.Encoding]::UTF8.GetBytes("chcp 65001 >nul`r" + 'echo ' + [char]0x754C + "^_CMD^_OK`r")
        $pipe.Write($command,0,$command.Length)
        $null = Read-Until $pipe $marker
        $results += [pscustomobject]@{Metric='cmd_unicode_roundtrip';Value=1}
    } finally { $pipe.Dispose() }
    for ($iteration=0; $iteration -lt 5; $iteration++) {
        $response = Request-Ferrux @{SpawnPane=@{shell='cmd.exe';cols=80;rows=24;cwd=$work}}
        $temporaryId = [long]$response.PaneSpawned.id
        $ids += $temporaryId
        $null = Request-Ferrux @{KillPane=@{id=$temporaryId}}
        $ids = @($ids | Where-Object { $_ -ne $temporaryId })
    }
    $results += [pscustomobject]@{Metric='rapid_create_kill_cycles';Value=5}
} finally {
    foreach ($id in $ids) { try { $null = Request-Ferrux @{KillPane=@{id=$id}} } catch { Write-Warning $_ } }
    if (!$daemon.HasExited) { Stop-Process -Id $daemon.Id }
    $daemon.Dispose()
    $env:FERRUX_PIPE_NAME = $previousFerruxPipe
}
$results | ConvertTo-Json | Set-Content -LiteralPath $OutputPath -Encoding UTF8
$results | Format-Table -AutoSize
