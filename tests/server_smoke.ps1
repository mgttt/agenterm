param(
    [string]$CtlExe = (Join-Path $PSScriptRoot '..\dist\agenterm-cli.exe'),
    [string]$ServerExe = (Join-Path $PSScriptRoot '..\dist\agenterm-server.exe'),
    [switch]$ListEvidence
)

$ErrorActionPreference = 'Stop'
$declaredEvidence = @(
    'server.headless-authority'
)
if ($ListEvidence) {
    $declaredEvidence
    exit 0
}

. (Join-Path $PSScriptRoot 'TestHarness.ps1')

$run = New-SmokeRunContext -Suite 'server' -Executable $CtlExe `
    -DeclaredEvidence $declaredEvidence -AllowPaneCapture
$CtlExe = $run.Executable
$ServerExe = [IO.Path]::GetFullPath($ServerExe)
if (-not (Test-Path -LiteralPath $ServerExe)) {
    throw "AgenTerm server executable not found: $ServerExe"
}

function Invoke-AgenTerm {
    param([string[]]$CommandArgs)
    Invoke-SmokeCli -Context $run -Arguments $CommandArgs
}

function Write-Evidence {
    param([Parameter(Mandatory = $true)][string]$Id)
    Write-SmokeEvidence -Context $run -Id $Id
}

$server = $null
$runSucceeded = $false
$runFailure = $null
try {
    Write-Host 'STEP start internal headless server'
    $server = Start-Process -FilePath $ServerExe `
        -ArgumentList @('--address', $run.Address) `
        -WindowStyle Hidden -PassThru
    Register-SmokeOwnedProcess -Context $run -Id $server.Id `
        -Kind 'server' -Address $run.Address

    $ready = $false
    $deadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        $output = @(
            & $CtlExe --address $run.Address protocol-info --running 2>&1
        )
        if ($LASTEXITCODE -eq 0) {
            $ready = $true
            break
        }
        Start-Sleep -Milliseconds 25
    } while ([DateTime]::UtcNow -lt $deadline)
    if (-not $ready) {
        throw "headless server did not become ready: $($output -join "`n")"
    }
    $protocol = Invoke-AgenTerm @('protocol-info', '--running') |
        ConvertFrom-Json
    $server.Refresh()
    if ($server.MainWindowHandle -ne 0 -or
        $protocol.pid -ne $server.Id -or
        $protocol.ui_bridge.ownership_mode -ne 'split_server_client' -or
        $protocol.ui_bridge.server_executable -ne 'agenterm-server.exe' -or
        $protocol.ui_bridge.replaceable_ui -or
        $protocol.ui_bridge.reconnect) {
        throw 'headless server did not publish its truthful process/ownership boundary'
    }

    Write-Host 'STEP create and observe a real PTY through public IPC'
    $hello = Invoke-AgenTerm @(
        'ui-hello', '--minimum', '1', '--maximum', '1',
        '--client-id', "server-smoke-$($run.RunId)"
    ) | ConvertFrom-Json
    $tabId = Invoke-AgenTerm @(
        'new-window', '-d', '-P', '-F', '#{window_id}',
        '-n', "headless-$($run.RunId)",
        '--', 'cmd.exe', '/d', '/c', 'echo AGENTERM_HEADLESS_OK'
    )
    if ($tabId -notmatch '^@\d+$') {
        throw 'headless server did not return a stable tab ID'
    }
    Invoke-AgenTerm @(
        'wait-pane', '-t', $tabId,
        '--contains', 'AGENTERM_HEADLESS_OK', '--timeout-ms', '5000'
    ) | Out-Null
    $capture = Invoke-AgenTerm @(
        'capture-pane', '-p', '-t', $tabId, '--max-bytes', '16384'
    )
    if (-not $capture.Contains('AGENTERM_HEADLESS_OK')) {
        throw 'headless server did not retain parsed terminal output'
    }

    Write-Host 'STEP follow server-owned events into terminal post-state'
    $note = "server-note-$($run.RunId)"
    Invoke-AgenTerm @('set-tab-note', '-t', $tabId, $note) | Out-Null
    $bootstrap = Invoke-AgenTerm @('ui-bootstrap') | ConvertFrom-Json
    $deltas = Invoke-AgenTerm @(
        'ui-deltas',
        '--epoch', $hello.position.server_epoch,
        '--after', "$($hello.position.sequence)",
        '--limit', '64'
    ) | ConvertFrom-Json
    $tab = @($bootstrap.tabs | Where-Object id -eq $tabId)
    $postState = @($deltas.tab_updates | Where-Object id -eq $tabId)
    $terminalEvent = @(
        $deltas.events |
            Where-Object {
                $_.tab_id -eq $tabId -and $_.kind -eq 'terminal.output'
            }
    )
    if ($hello.server_pid -ne $server.Id -or
        $bootstrap.server_pid -ne $server.Id -or
        $tab.Count -ne 1 -or $tab[0].note -ne $note -or
        $postState.Count -ne 1 -or $postState[0].note -ne $note -or
        $terminalEvent.Count -eq 0 -or
        -not $deltas.complete -or $deltas.truncated) {
        throw 'headless server did not preserve causal Fleet, PTY, and post-state truth'
    }

    Write-Host 'STEP persist and stop server without a GUI process'
    Invoke-AgenTerm @('save-workspace') | Out-Null
    if (-not (Test-Path -LiteralPath $run.WorkspacePath)) {
        throw 'headless server did not persist its workspace'
    }
    Write-Evidence 'server.headless-authority'
    Invoke-AgenTerm @('shutdown') | Out-Null
    $server.WaitForExit(10000) | Out-Null
    if (-not $server.HasExited) {
        throw 'headless server did not exit after graceful shutdown'
    }
    $runSucceeded = $true
}
catch {
    $runFailure = $_
}
finally {
    Complete-SmokeRun -Context $run -Succeeded $runSucceeded `
        -FailureRecord $runFailure
}

if (-not $runSucceeded) {
    throw $runFailure
}
Write-Host 'PASS: headless server owns PTY, parser, workspace, events, and no HWND'
