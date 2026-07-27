param(
    [string]$GuiExe = (Join-Path $PSScriptRoot '..\dist\agenterm.exe'),
    [string]$CliExe = (Join-Path $PSScriptRoot '..\dist\agenterm-cli.exe'),
    [int]$MaxWindowMs = 1000,
    [switch]$ListEvidence
)

$ErrorActionPreference = 'Stop'
$declaredEvidence = @('startup.first-window-async-ready')
if ($ListEvidence) {
    $declaredEvidence
    exit 0
}

. (Join-Path $PSScriptRoot 'TestHarness.ps1')

$GuiExe = [IO.Path]::GetFullPath($GuiExe)
$CliExe = [IO.Path]::GetFullPath($CliExe)
foreach ($path in @($GuiExe, $CliExe)) {
    if (-not (Test-Path -LiteralPath $path)) {
        throw "AgenTerm executable not found: $path"
    }
}
if ($MaxWindowMs -le 0) {
    throw 'MaxWindowMs must be a positive integer.'
}

$run = New-SmokeRunContext -Suite 'startup' -Executable $CliExe `
    -DeclaredEvidence $declaredEvidence
$CliExe = $run.Executable
$ownedProcesses = [Collections.Generic.List[Diagnostics.Process]]::new()
$runSucceeded = $false
$runFailure = $null

function Invoke-AgenTerm {
    param([string[]]$CommandArgs)
    Invoke-SmokeCli -Context $run -Arguments $CommandArgs
}

function Add-OwnedProcess {
    param([Parameter(Mandatory = $true)][Diagnostics.Process]$Process)
    $ownedProcesses.Add($Process)
    return $Process
}

function Stop-RemainingOwnedProcesses {
    $cleanupFailures = [Collections.Generic.List[string]]::new()
    foreach ($ownedProcess in $ownedProcesses) {
        try {
            $ownedProcess.Refresh()
            if (-not $ownedProcess.HasExited -and
                -not $ownedProcess.WaitForExit(2000)) {
                Stop-Process -Id $ownedProcess.Id -Force -ErrorAction Stop
                if (-not $ownedProcess.WaitForExit(2000)) {
                    Write-Warning (
                        "Owned startup process PID $($ownedProcess.Id) " +
                        'did not exit after forced cleanup.'
                    )
                }
            }
        }
        catch {
            $message = (
                "Unable to finish cleanup for owned startup process " +
                "PID $($ownedProcess.Id): $($_.Exception.Message)"
            )
            $cleanupFailures.Add($message)
            Write-Warning $message
        }
        finally {
            $ownedProcess.Dispose()
        }
    }
    if ($runSucceeded -and $cleanupFailures.Count -gt 0) {
        throw ($cleanupFailures -join "`n")
    }
}

$stderrFile = Join-Path $run.RunDirectory 'gui-startup-stderr.txt'
$secondStderrFile = Join-Path $run.RunDirectory 'gui-handoff-stderr.txt'
$guidanceStdoutFile = Join-Path $run.RunDirectory 'gui-guidance-stdout.txt'
$guidanceStderrFile = Join-Path $run.RunDirectory 'gui-guidance-stderr.txt'
$argumentStdoutFile = Join-Path $run.RunDirectory 'gui-argument-stdout.txt'
$argumentStderrFile = Join-Path $run.RunDirectory 'gui-argument-stderr.txt'
$workerIdsBefore = @(
    Get-Process -Name 'agenterm-script' -ErrorAction SilentlyContinue |
        ForEach-Object { $_.Id }
)

try {
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $process = Add-OwnedProcess (
        Start-Process -FilePath $GuiExe `
            -RedirectStandardError $stderrFile `
            -PassThru
    )
    $discoveryDeadline = [DateTime]::UtcNow.AddSeconds(5)
    $handle = [IntPtr]::Zero
    while ([DateTime]::UtcNow -lt $discoveryDeadline) {
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
        throw (
            "AgenTerm first window took $($watch.ElapsedMilliseconds) ms; " +
            "limit is $MaxWindowMs ms."
        )
    }

    $version = ((Invoke-AgenTerm @('--version')) -split '\s+')[-1]
    $process.Refresh()
    $ipcPort = ($run.Address -split ':')[-1]
    if ($process.MainWindowTitle -ne "AgenTerm-$version`:$ipcPort") {
        throw "Unexpected window title: $($process.MainWindowTitle)"
    }

    # The one-second budget ends at native-window discovery. ConPTY startup is
    # deliberately asynchronous and is awaited through public structured state.
    Invoke-AgenTerm @(
        'wait-ui', '-t', '0', '--tab-state', 'running',
        '--timeout-ms', '10000'
    ) | Out-Null

    $unexpectedWorkers = @(
        Get-Process -Name 'agenterm-script' -ErrorAction SilentlyContinue |
            Where-Object { $workerIdsBefore -notcontains $_.Id }
    )
    if ($unexpectedWorkers.Count -ne 0) {
        throw (
            'Normal GUI startup unexpectedly launched script worker PID(s): ' +
            ($unexpectedWorkers.Id -join ', ')
        )
    }

    $startupStderr = Get-Content -LiteralPath $stderrFile -Raw
    $expectedStartupStderr = @(
        "Launcher PID: $($process.Id)"
        "Configured server address: $($run.Address)"
        ''
        'List running server PID and port: agenterm-cli.exe server-list'
        'More CLI commands: agenterm-cli.exe -h'
    ) -join "`n"
    if ($startupStderr.TrimEnd() -ne $expectedStartupStderr) {
        throw "GUI inherited-stderr guidance was incomplete:`n$startupStderr"
    }

    $second = Add-OwnedProcess (
        Start-Process -FilePath $GuiExe `
            -RedirectStandardError $secondStderrFile `
            -PassThru
    )
    if (-not $second.WaitForExit(1000)) {
        throw 'A second GUI launch did not hand off to the existing instance.'
    }
    $focusStderr = Get-Content -LiteralPath $secondStderrFile -Raw
    $expectedFocusStderr = @(
        "Launcher PID: $($second.Id)"
        "Configured server address: $($run.Address)"
        ''
        'List running server PID and port: agenterm-cli.exe server-list'
        'More CLI commands: agenterm-cli.exe -h'
    ) -join "`n"
    if ($focusStderr.TrimEnd() -ne $expectedFocusStderr) {
        throw "existing-GUI inherited-stderr guidance was incomplete:`n$focusStderr"
    }

    $guidance = Add-OwnedProcess (
        Start-Process -FilePath $GuiExe `
            -ArgumentList @('list-windows') `
            -RedirectStandardOutput $guidanceStdoutFile `
            -RedirectStandardError $guidanceStderrFile `
            -PassThru
    )
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

    $argumentError = Add-OwnedProcess (
        Start-Process -FilePath $GuiExe `
            -ArgumentList @('--address') `
            -RedirectStandardOutput $argumentStdoutFile `
            -RedirectStandardError $argumentStderrFile `
            -PassThru
    )
    if (-not $argumentError.WaitForExit(1000)) {
        throw 'Invalid GUI arguments blocked instead of returning an error'
    }
    $argumentError.Refresh()
    $argumentStdout = Get-Content -LiteralPath $argumentStdoutFile -Raw
    $argumentStderr = Get-Content -LiteralPath $argumentStderrFile -Raw
    if ($argumentError.ExitCode -eq 0 -or
        -not [string]::IsNullOrEmpty($argumentStdout) -or
        -not $argumentStderr.Contains('AgenTerm GUI argument error') -or
        -not $argumentStderr.Contains(
            'agenterm.exe --address requires HOST:PORT'
        ) -or
        -not $argumentStderr.Contains('agenterm-cli.exe -h')) {
        throw "nonblocking GUI argument error was incorrect:`n$argumentStderr"
    }

    Write-SmokeEvidence -Context $run -Id $declaredEvidence[0]
    Write-Host (
        "PASS: first window in $($watch.ElapsedMilliseconds) ms; " +
        'terminal loaded asynchronously'
    )
    $runSucceeded = $true
}
catch {
    $runFailure = $_
    throw
}
finally {
    try {
        Complete-SmokeRun -Context $run -Succeeded $runSucceeded `
            -FailureRecord $runFailure
    }
    finally {
        Stop-RemainingOwnedProcesses
    }
}
