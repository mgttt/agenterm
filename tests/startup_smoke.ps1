param(
    [string]$GuiExe = (Join-Path $PSScriptRoot '..\dist\agenterm.exe'),
    [string]$CliExe = (Join-Path $PSScriptRoot '..\dist\agentermctl.exe'),
    [int]$MaxWindowMs = 1000
)

$ErrorActionPreference = 'Stop'
$GuiExe = [IO.Path]::GetFullPath($GuiExe)
$CliExe = [IO.Path]::GetFullPath($CliExe)
foreach ($path in @($GuiExe, $CliExe)) {
    if (-not (Test-Path -LiteralPath $path)) {
        throw "AgenTerm executable not found: $path"
    }
}

$previousAddress = $env:AGENTERM_IPC_ADDRESS
$previousWorkspace = $env:AGENTERM_WORKSPACE_PATH
$env:AGENTERM_IPC_ADDRESS = "127.0.0.1:$((46000 + ($PID % 1000)))"
$workspaceFile = Join-Path $env:TEMP "agenterm-startup-$PID.json"
$env:AGENTERM_WORKSPACE_PATH = $workspaceFile
& $CliExe kill-server 2>$null | Out-Null
Start-Sleep -Milliseconds 200

try {
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $process = Start-Process -FilePath $GuiExe -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    $handle = [IntPtr]::Zero
    while ([DateTime]::UtcNow -lt $deadline) {
        $process.Refresh()
        $handle = $process.MainWindowHandle
        if ($handle -ne [IntPtr]::Zero) {
            break
        }
        Start-Sleep -Milliseconds 5
    }
    $watch.Stop()

    if ($handle -eq [IntPtr]::Zero) {
        throw 'AgenTerm did not expose its main window within 5 seconds.'
    }
    if ($watch.ElapsedMilliseconds -gt $MaxWindowMs) {
        throw "AgenTerm first window took $($watch.ElapsedMilliseconds) ms; limit is $MaxWindowMs ms."
    }
    $version = ((& $CliExe --version) -split '\s+')[-1]
    $process.Refresh()
    $ipcPort = ($env:AGENTERM_IPC_ADDRESS -split ':')[-1]
    if ($process.MainWindowTitle -ne "AgenTerm-$version`:$ipcPort") {
        throw "Unexpected window title: $($process.MainWindowTitle)"
    }

    & $CliExe wait-ui -t 0 --tab-state running --timeout-ms 10000 | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw 'The asynchronously started initial terminal did not become ready.'
    }

    $second = Start-Process -FilePath $GuiExe -PassThru
    if (-not $second.WaitForExit(1000)) {
        throw 'A second GUI launch did not hand off to the existing instance.'
    }

    Write-Host "PASS: first window in $($watch.ElapsedMilliseconds) ms; terminal loaded asynchronously"
}
finally {
    & $CliExe kill-server 2>$null | Out-Null
    Remove-Item -LiteralPath $workspaceFile -ErrorAction SilentlyContinue
    if ($null -eq $previousAddress) {
        Remove-Item Env:AGENTERM_IPC_ADDRESS -ErrorAction SilentlyContinue
    } else {
        $env:AGENTERM_IPC_ADDRESS = $previousAddress
    }
    if ($null -eq $previousWorkspace) {
        Remove-Item Env:AGENTERM_WORKSPACE_PATH -ErrorAction SilentlyContinue
    } else {
        $env:AGENTERM_WORKSPACE_PATH = $previousWorkspace
    }
}
