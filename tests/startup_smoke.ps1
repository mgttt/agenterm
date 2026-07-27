param(
    [string]$GuiExe = (Join-Path $PSScriptRoot '..\dist\agenterm.exe'),
    [string]$CliExe = (Join-Path $PSScriptRoot '..\dist\agenterm-cli.exe'),
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
$stderrFile = Join-Path $env:TEMP "agenterm-startup-stderr-$PID.txt"
$secondStderrFile = Join-Path $env:TEMP "agenterm-focus-stderr-$PID.txt"
$guidanceStdoutFile = Join-Path $env:TEMP "agenterm-guidance-stdout-$PID.txt"
$guidanceStderrFile = Join-Path $env:TEMP "agenterm-guidance-stderr-$PID.txt"
$argumentStdoutFile = Join-Path $env:TEMP "agenterm-argument-stdout-$PID.txt"
$argumentStderrFile = Join-Path $env:TEMP "agenterm-argument-stderr-$PID.txt"
$env:AGENTERM_WORKSPACE_PATH = $workspaceFile
& $CliExe kill-server 2>$null | Out-Null
Start-Sleep -Milliseconds 200

try {
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $process = Start-Process -FilePath $GuiExe -RedirectStandardError $stderrFile -PassThru
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
    $startupStderr = Get-Content -LiteralPath $stderrFile -Raw
    if (-not $startupStderr.Contains("GUI PID: $($process.Id)") -or
        -not $startupStderr.Contains("Server address: $($env:AGENTERM_IPC_ADDRESS) (port $ipcPort)") -or
        -not $startupStderr.Contains('agenterm-cli.exe server-list') -or
        -not $startupStderr.Contains('agenterm-cli.exe -h')) {
        throw "GUI inherited-stderr guidance was incomplete:`n$startupStderr"
    }

    $second = Start-Process -FilePath $GuiExe -RedirectStandardError $secondStderrFile -PassThru
    if (-not $second.WaitForExit(1000)) {
        throw 'A second GUI launch did not hand off to the existing instance.'
    }
    $focusStderr = Get-Content -LiteralPath $secondStderrFile -Raw
    if (-not $focusStderr.Contains("Focused the existing AgenTerm GUI server at $($env:AGENTERM_IPC_ADDRESS)") -or
        -not $focusStderr.Contains('agenterm-cli.exe server-list') -or
        -not $focusStderr.Contains('agenterm-cli.exe -h')) {
        throw "existing-GUI inherited-stderr guidance was incomplete:`n$focusStderr"
    }

    $guidance = Start-Process -FilePath $GuiExe `
        -ArgumentList @('list-windows') `
        -RedirectStandardOutput $guidanceStdoutFile `
        -RedirectStandardError $guidanceStderrFile `
        -PassThru
    if (-not $guidance.WaitForExit(1000)) {
        throw 'CLI-style agenterm.exe invocation blocked instead of returning guidance'
    }
    $guidance.Refresh()
    $guidanceStdout = Get-Content -LiteralPath $guidanceStdoutFile -Raw
    $guidanceStderr = Get-Content -LiteralPath $guidanceStderrFile -Raw
    if ($guidance.ExitCode -eq 0 -or
        -not [string]::IsNullOrEmpty($guidanceStdout) -or
        -not $guidanceStderr.Contains('No CLI command was executed') -or
        -not $guidanceStderr.Contains('agenterm-cli.exe list-windows') -or
        -not $guidanceStderr.Contains('agenterm-cli.exe server-list') -or
        -not $guidanceStderr.Contains('agenterm-cli.exe -h')) {
        throw "nonblocking GUI CLI guidance was incorrect:`n$guidanceStderr"
    }

    $argumentError = Start-Process -FilePath $GuiExe `
        -ArgumentList @('--address') `
        -RedirectStandardOutput $argumentStdoutFile `
        -RedirectStandardError $argumentStderrFile `
        -PassThru
    if (-not $argumentError.WaitForExit(1000)) {
        throw 'Invalid GUI arguments blocked instead of returning an error'
    }
    $argumentError.Refresh()
    $argumentStdout = Get-Content -LiteralPath $argumentStdoutFile -Raw
    $argumentStderr = Get-Content -LiteralPath $argumentStderrFile -Raw
    if ($argumentError.ExitCode -eq 0 -or
        -not [string]::IsNullOrEmpty($argumentStdout) -or
        -not $argumentStderr.Contains('AgenTerm GUI argument error') -or
        -not $argumentStderr.Contains('agenterm.exe --address requires HOST:PORT') -or
        -not $argumentStderr.Contains('agenterm-cli.exe -h')) {
        throw "nonblocking GUI argument error was incorrect:`n$argumentStderr"
    }

    Write-Host "PASS: first window in $($watch.ElapsedMilliseconds) ms; terminal loaded asynchronously"
}
finally {
    & $CliExe kill-server 2>$null | Out-Null
    Remove-Item -LiteralPath $workspaceFile -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $stderrFile -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $secondStderrFile -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $guidanceStdoutFile -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $guidanceStderrFile -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $argumentStdoutFile -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $argumentStderrFile -ErrorAction SilentlyContinue
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
