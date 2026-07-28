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

function Invoke-AgenTermExpectedFailure {
    param([string[]]$CommandArgs)
    Invoke-SmokeCli -Context $run -Arguments $CommandArgs -ExpectFailure
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
        -not $protocol.ui_bridge.replaceable_ui -or
        -not $protocol.ui_bridge.interactive_lease -or
        -not $protocol.ui_bridge.reconnect -or
        -not $protocol.ui_bridge.rollback_proven) {
        throw 'headless server did not publish its truthful process/ownership boundary'
    }

    Write-Host 'STEP acquire, renew, conflict, heartbeat, and detach the UI lease'
    $leaseClientId = "server-smoke-ui-$($run.RunId)"
    $attachArgs = @(
        'ui-lease', 'attach',
        '--client-id', $leaseClientId,
        '--client-pid', "$PID"
    )
    $lease = Invoke-AgenTerm $attachArgs | ConvertFrom-Json
    $sameLease = Invoke-AgenTerm $attachArgs | ConvertFrom-Json
    $conflict = Invoke-AgenTermExpectedFailure @(
        'ui-lease', 'attach',
        '--client-id', "conflict-$($run.RunId)",
        '--client-pid', "$($server.Id)"
    )
    if ($lease.lease_id -notmatch '^ui-[a-f0-9-]+$' -or
        $sameLease.lease_id -ne $lease.lease_id -or
        $sameLease.expires_unix_ms -lt $lease.expires_unix_ms -or
        -not $conflict.Contains('another live UI client')) {
        throw 'headless server did not enforce one idempotent live UI lease owner'
    }
    $heartbeat = Invoke-AgenTerm @(
        'ui-lease', 'heartbeat',
        '--lease-id', $lease.lease_id,
        '--client-pid', "$PID"
    ) | ConvertFrom-Json
    if ($heartbeat.lease_id -ne $lease.lease_id -or
        $heartbeat.client_pid -ne $PID) {
        throw 'headless server did not renew the exact UI lease owner'
    }

    Write-Host 'STEP require the lease for UI select, resize, and binary input'
    $interactionTab = Invoke-AgenTerm @(
        'new-window', '-d', '-P', '-F', '#{window_id}',
        '-n', "interaction-$($run.RunId)",
        '--', 'cmd.exe', '/d', '/q'
    )
    $unauthorized = Invoke-AgenTermExpectedFailure @(
        'ui-interact', 'select',
        '--lease-id', 'wrong-lease',
        '--client-pid', "$PID",
        '-t', $interactionTab
    )
    $selected = Invoke-AgenTerm @(
        'ui-interact', 'select',
        '--lease-id', $lease.lease_id,
        '--client-pid', "$PID",
        '-t', $interactionTab
    ) | ConvertFrom-Json
    $resized = Invoke-AgenTerm @(
        'ui-interact', 'resize',
        '--lease-id', $lease.lease_id,
        '--client-pid', "$PID",
        '-t', $interactionTab,
        '--rows', '24', '--columns', '90'
    ) | ConvertFrom-Json
    $interactionMarker = "AGENTERM_UI_INPUT_$($run.RunId)"
    $inputText = "echo $interactionMarker`r"
    $inputHex = ([BitConverter]::ToString(
            [Text.Encoding]::UTF8.GetBytes($inputText)
        )).Replace('-', '').ToLowerInvariant()
    $input = Invoke-AgenTerm @(
        'ui-interact', 'input',
        '--lease-id', $lease.lease_id,
        '--client-pid', "$PID",
        '-t', $interactionTab,
        '--hex', $inputHex
    ) | ConvertFrom-Json
    Invoke-AgenTerm @(
        'wait-pane', '-t', $interactionTab,
        '--contains', $interactionMarker, '--timeout-ms', '5000'
    ) | Out-Null
    $interactionBootstrap = Invoke-AgenTerm @('ui-bootstrap') |
        ConvertFrom-Json
    $interactionState = $interactionBootstrap.tabs |
        Where-Object id -eq $interactionTab |
        Select-Object -First 1
    if (-not $unauthorized.Contains('does not match the current owner') -or
        $selected.action -ne 'select' -or
        $selected.tab_id -ne $interactionTab -or
        $resized.rows -ne 24 -or $resized.columns -ne 90 -or
        $input.input_bytes -ne [Text.Encoding]::UTF8.GetByteCount($inputText) -or
        $interactionBootstrap.active_tab_id -ne $interactionTab -or
        $interactionState.screen.rows -ne 24 -or
        $interactionState.screen.columns -ne 90) {
        throw 'headless server did not enforce lease-gated UI interaction'
    }
    $acknowledged = Invoke-AgenTerm @(
        'ui-lease', 'acknowledge',
        '--lease-id', $lease.lease_id,
        '--client-pid', "$PID",
        '--sequence', "$($interactionBootstrap.position.sequence)"
    ) | ConvertFrom-Json
    $regression = Invoke-AgenTermExpectedFailure @(
        'ui-lease', 'acknowledge',
        '--lease-id', $lease.lease_id,
        '--client-pid', "$PID",
        '--sequence', "$($interactionBootstrap.position.sequence - 1)"
    )
    if ($acknowledged.observed_sequence -ne
            $interactionBootstrap.position.sequence -or
        -not $regression.Contains('advance monotonically')) {
        throw 'headless server did not retain a monotonic UI observation position'
    }

    $detached = Invoke-AgenTerm @(
        'ui-lease', 'detach',
        '--lease-id', $lease.lease_id,
        '--client-pid', "$PID"
    ) | ConvertFrom-Json
    $leaseStatus = Invoke-AgenTerm @('ui-lease', 'status') |
        ConvertFrom-Json
    if (-not $detached.detached -or $leaseStatus.attached) {
        throw 'headless server did not explicitly release the UI lease'
    }

    Write-Host 'STEP create and observe a real PTY through public IPC'
    $hello = Invoke-AgenTerm @(
        'ui-hello', '--minimum', '1', '--maximum', '1',
        '--client-id', "server-smoke-$($run.RunId)"
    ) | ConvertFrom-Json
    if (-not $hello.capabilities.Contains('interactive_lease') -or
        -not $hello.capabilities.Contains('lease_gated_interaction') -or
        -not $hello.capabilities.Contains('replaceable_ui_client') -or
        -not $hello.capabilities.Contains('lease_owned_client_state') -or
        -not $hello.capabilities.Contains('in_place_reconnect')) {
        throw 'headless server hello did not discover its interactive contracts'
    }
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

    Write-Host 'STEP prove server-owned receipt replay and asynchronous completion'
    $receiptRequestId = "server-receipt-$($run.RunId)"
    $receiptArgs = @(
        '--request-id', $receiptRequestId, '--receipt-json',
        'set-tab-note', '-t', $tabId, "receipt-note-$($run.RunId)"
    )
    $firstReceipt = Invoke-AgenTerm $receiptArgs | ConvertFrom-Json
    $replayedReceipt = Invoke-AgenTerm $receiptArgs | ConvertFrom-Json
    $conflictReceipt = Invoke-AgenTermExpectedFailure @(
        '--request-id', $receiptRequestId, '--receipt-json',
        'set-tab-note', '-t', $tabId, 'different-note'
    ) | ConvertFrom-Json
    if ($firstReceipt.outcome -ne 'committed' -or
        $firstReceipt.request_id -ne $receiptRequestId -or
        $firstReceipt.after_position.sequence -ne
            $replayedReceipt.after_position.sequence -or
        $conflictReceipt.outcome -ne 'no_op' -or
        $conflictReceipt.error.code -ne 'request_id_conflict') {
        throw 'headless server did not own committed replay and conflict rejection'
    }

    $bootstrap = Invoke-AgenTerm @('ui-bootstrap') | ConvertFrom-Json
    $interactiveTab = [string]$bootstrap.active_tab_id
    $submissionMarker = "AGENTERM_SERVER_RECEIPT_$($run.RunId)"
    Invoke-AgenTerm @(
        'set-composer', '-t', $interactiveTab, "echo $submissionMarker"
    ) | Out-Null
    $submissionRequestId = "server-submit-$($run.RunId)"
    $submissionArgs = @(
        '--request-id', $submissionRequestId, '--receipt-json',
        'send-composer', '-t', $interactiveTab
    )
    $acceptedReceipt = Invoke-AgenTerm $submissionArgs | ConvertFrom-Json
    if ($acceptedReceipt.outcome -ne 'accepted' -or
        $acceptedReceipt.wait.condition -ne 'submission_complete') {
        throw 'headless server did not return an asynchronous accepted receipt'
    }
    Invoke-AgenTerm @(
        'wait-pane', '-t', $interactiveTab,
        '--contains', $submissionMarker, '--submit-complete',
        '--timeout-ms', '5000'
    ) | Out-Null
    $committedReceipt = Invoke-AgenTerm $submissionArgs | ConvertFrom-Json
    if ($committedReceipt.outcome -ne 'committed' -or
        $committedReceipt.wait -or
        $committedReceipt.after_position.sequence -le
            $acceptedReceipt.after_position.sequence) {
        throw 'headless server did not finalize the accepted receipt after terminal input'
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
