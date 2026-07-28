param(
    [string]$Exe = (Join-Path $PSScriptRoot '..\dist\agenterm-cli.exe'),
    [switch]$ListEvidence
)

$ErrorActionPreference = 'Stop'
$declaredEvidence = @(
    'cli.backspace-del-one'
    'cli.remain-on-exit'
    'cli.stable-create-id'
    'cli.observable-events'
    'cli.typed-tabs-operations'
    'cli.control-receipts'
    'cli.ui-bridge-contracts'
    'cli.ui-bootstrap'
    'cli.ui-follow'
)
if ($ListEvidence) {
    $declaredEvidence
    exit 0
}

. (Join-Path $PSScriptRoot 'TestHarness.ps1')

function Write-Evidence {
    param([Parameter(Mandatory = $true)][string]$Id)
    Write-SmokeEvidence -Context $run -Id $Id
}

$run = New-SmokeRunContext -Suite 'cli' -Executable $Exe `
    -DeclaredEvidence $declaredEvidence -AllowPaneCapture
$Exe = $run.Executable

function Invoke-AgenTerm {
    param([string[]]$CommandArgs)
    Invoke-SmokeCli -Context $run -Arguments $CommandArgs
}

function Invoke-AgenTermExpectedFailure {
    param([string[]]$CommandArgs)
    Invoke-SmokeCli -Context $run -Arguments $CommandArgs -ExpectFailure
}

$name = "agenterm-smoke-$($run.RunId)"
$token = "AGENTERM_SMOKE_$($run.RunId)"
$targetDir = $run.RunDirectory
$windowPng = Join-Path $targetDir "$name-window.png"
$panePng = Join-Path $targetDir "$name-pane.png"
$created = $false
$runSucceeded = $false
$runFailure = $null

try {
    Write-Host 'STEP create tab'
    Invoke-AgenTerm @('new-window', '-d', '-n', $name) | Out-Null
    $created = $true

    Write-Host 'STEP request receipts and replay protection'
    $requestId = "receipt-$($run.RunId)"
    $receiptArgs = @(
        '--request-id', $requestId, '--receipt-json',
        'set-tab-note', '-t', $name, 'receipt-test'
    )
    $firstReceipt = Invoke-AgenTerm $receiptArgs | ConvertFrom-Json
    $replayedReceipt = Invoke-AgenTerm $receiptArgs | ConvertFrom-Json
    if (
        $firstReceipt.outcome -ne 'committed' -or
        $firstReceipt.request_id -ne $requestId -or
        $firstReceipt.resolved.tab_id -notmatch '^\d+$' -or
        $firstReceipt.after_position.sequence -ne
            $replayedReceipt.after_position.sequence
    ) {
        throw 'Control receipt did not preserve committed replay identity'
    }
    $conflictReceipt = Invoke-AgenTermExpectedFailure @(
        '--request-id', $requestId, '--receipt-json',
        'set-tab-note', '-t', $name, 'different-payload'
    ) | ConvertFrom-Json
    if (
        $conflictReceipt.outcome -ne 'no_op' -or
        $conflictReceipt.error.code -ne 'request_id_conflict' -or
        $conflictReceipt.error.category -ne 'conflict'
    ) {
        throw 'Request ID reuse with a different payload was not rejected'
    }
    $retryName = "receipt-window-$($run.RunId)"
    $createRequestId = "receipt-create-$($run.RunId)"
    $createArgs = @(
        '--request-id', $createRequestId, '--receipt-json',
        'new-window', '-d', '-n', $retryName
    )
    $createReceipt = Invoke-AgenTerm $createArgs | ConvertFrom-Json
    $createReplay = Invoke-AgenTerm $createArgs | ConvertFrom-Json
    $retryTabs = @(
        (Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json).tabs |
            Where-Object name -eq $retryName
    )
    if (
        $createReceipt.outcome -ne 'committed' -or
        $createReceipt.request_id -ne $createRequestId -or
        $createReceipt.after_position.sequence -ne
            $createReplay.after_position.sequence -or
        $retryTabs.Count -ne 1
    ) {
        throw 'Retried new-window did not commit exactly one stable tab'
    }
    $retryTabId = $retryTabs[0].id
    if ($retryTabId -notmatch '^@\d+$') {
        throw 'Retried new-window did not expose a stable tab ID'
    }
    $killRequestId = "receipt-kill-$($run.RunId)"
    $killArgs = @(
        '--request-id', $killRequestId, '--receipt-json',
        'kill-window', '-t', $retryTabId
    )
    $killReceipt = Invoke-AgenTerm $killArgs | ConvertFrom-Json
    $killReplay = Invoke-AgenTerm $killArgs | ConvertFrom-Json
    $retryTabsAfterKill = @(
        (Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json).tabs |
            Where-Object id -eq $retryTabId
    )
    if (
        $killReceipt.outcome -ne 'committed' -or
        $killReceipt.request_id -ne $killRequestId -or
        $killReceipt.after_position.sequence -ne
            $killReplay.after_position.sequence -or
        $retryTabsAfterKill.Count -ne 0
    ) {
        throw 'Retried kill-window did not replay its committed terminal close'
    }
    Write-Evidence 'cli.control-receipts'

    Write-Host 'STEP discover aligned and extended commands'
    $commands = Invoke-AgenTerm @('list-commands')
    foreach ($expected in @(
        'new-window (neww)',
        'list-windows (lsw)',
        'send-keys (send)',
        'scroll-pane',
        'read-events',
        'wait-events',
        'new-agent',
        'list-instances',
        'wait-pane (expect-pane)',
        'set-composer',
        'ui-bootstrap',
        'ui-deltas',
        'ui-hello',
        'ui-snapshot'
    )) {
        if (-not $commands.Contains($expected)) {
            throw "list-commands did not advertise: $expected"
        }
    }

    Write-Host 'STEP typed operation discovery, Tabs semantics, and stable errors'
    $liveAddress = $env:AGENTERM_IPC_ADDRESS
    try {
        $env:AGENTERM_IPC_ADDRESS = '127.0.0.1:1'
        $offlineTypedError = Invoke-AgenTermExpectedFailure @(
            'ui-action', 'tabs-teleport'
        )
        $offlineWidthError = Invoke-AgenTermExpectedFailure @(
            'ui-action', 'tabs-set-width'
        )
    }
    finally {
        $env:AGENTERM_IPC_ADDRESS = $liveAddress
    }
    if (-not $offlineTypedError.Contains(
        'operation_unknown[tabs-teleport]'
    ) -or $offlineTypedError.Contains('connect')) {
        throw 'typed operation validation did not fail offline before IPC discovery'
    }
    if (-not $offlineWidthError.Contains(
        'operation_invalid_arguments[ui.tabs.set-width]'
    ) -or $offlineWidthError.Contains('connect')) {
        throw 'typed Tabs width validation did not fail offline before IPC discovery'
    }

    $protocol = Invoke-AgenTerm @('protocol-info') | ConvertFrom-Json
    $operationCatalog = $protocol.operation_catalog
    if ($protocol.ui_bridge.schema_version -ne 3 -or
        $protocol.ui_bridge.ownership_mode -ne 'combined_gui_server' -or
        $protocol.ui_bridge.replaceable_ui -or
        $protocol.ui_bridge.interactive_lease -or
        $protocol.ui_bridge.target_server_executable -ne 'agenterm-server.exe' -or
        -not $protocol.ui_bridge.bootstrap_snapshot -or
        -not $protocol.ui_bridge.ordered_deltas -or
        $protocol.ui_bridge.reconnect -or
        $protocol.ui_bridge.contract_schemas.hello -ne 1 -or
        $protocol.ui_bridge.contract_schemas.bootstrap -ne 1 -or
        $protocol.ui_bridge.contract_schemas.screen -ne 1 -or
        $protocol.ui_bridge.contract_schemas.delta -ne 1 -or
        $protocol.ui_bridge.contract_schemas.lease -ne 1 -or
        $protocol.ui_bridge.hard_limits.bootstrap_bytes -ne 8388608 -or
        $protocol.ui_bridge.hard_limits.tabs -ne 1024 -or
        $protocol.ui_bridge.hard_limits.screen_rows -ne 512 -or
        $protocol.ui_bridge.hard_limits.screen_columns -ne 512 -or
        $protocol.ui_bridge.hard_limits.screen_runs -ne 262144 -or
        $protocol.ui_bridge.hard_limits.screen_text_bytes -ne 4194304 -or
        $protocol.ui_bridge.hard_limits.delta_bytes -ne 8388608 -or
        $protocol.ui_bridge.hard_limits.delta_events -ne 64 -or
        $operationCatalog.schema_version -ne 1 -or
        -not $operationCatalog.classification_only -or
        $operationCatalog.authorization_policy) {
        throw 'protocol-info did not expose truthful UI ownership and operation facts'
    }
    Write-Evidence 'cli.ui-bridge-contracts'
    Write-Host 'STEP renderer-neutral server bootstrap projection'
    $runningProtocol = Invoke-AgenTerm @('protocol-info', '--running') | ConvertFrom-Json
    $bootstrap = Invoke-AgenTerm @('ui-bootstrap') | ConvertFrom-Json
    $inspect = Invoke-AgenTerm @('inspect') | ConvertFrom-Json
    $uiSnapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    if ($bootstrap.schema_version -ne 1 -or
        $runningProtocol.identity_scope -ne 'running_host' -or
        $bootstrap.server_pid -ne $runningProtocol.pid -or
        $bootstrap.server_epoch -ne $bootstrap.position.server_epoch -or
        $bootstrap.server_epoch -ne $uiSnapshot.event_position.epoch -or
        $bootstrap.position.sequence -ne $uiSnapshot.event_position.sequence -or
        -not $bootstrap.complete -or $bootstrap.truncated -or
        $bootstrap.active_tab_id -ne $inspect.active_window_id -or
        @($bootstrap.tabs).Count -ne @($inspect.windows).Count) {
        throw 'ui-bootstrap did not preserve causal server/workspace identity'
    }
    foreach ($tab in @($bootstrap.tabs)) {
        $window = @($inspect.windows | Where-Object id -eq $tab.id)
        if ($window.Count -ne 1 -or
            $tab.parent_id -ne $window[0].parent_id -or
            $tab.title -ne $window[0].name -or
            $tab.note -ne $window[0].note -or
            $tab.process_id -ne $window[0].pid -or
            $tab.dead -ne $window[0].dead -or
            $tab.exit_code -ne $window[0].exit_code -or
            $tab.screen.tab_id -ne $tab.id -or
            $tab.screen.rows -ne $window[0].rows -or
            $tab.screen.columns -ne $window[0].cols -or
            $tab.screen.scrollback_offset -ne $window[0].scrollback_offset -or
            -not $tab.screen.complete -or $tab.screen.truncated -or
            @($tab.screen.runs).Count -eq 0) {
            throw "ui-bootstrap tab projection drifted from inspect for $($tab.id)"
        }
        if ($tab.composer.sensitive) {
            if ($null -ne $tab.composer.text) {
                throw 'ui-bootstrap disclosed a sensitive composer draft'
            }
        }
        elseif ($tab.composer.byte_length -ne
            [Text.Encoding]::UTF8.GetByteCount([string]$tab.composer.text)) {
            throw 'ui-bootstrap composer byte identity was inconsistent'
        }
        $workingProperties = @($tab.working_context.PSObject.Properties.Name)
        if ($workingProperties -contains 'http_proxy' -or
            $workingProperties -contains 'https_proxy') {
            throw 'ui-bootstrap exposed proxy values instead of redacted facts'
        }
    }
    Write-Evidence 'cli.ui-bootstrap'

    Write-Host 'STEP UI handshake and causal snapshot-follow'
    $hello = Invoke-AgenTerm @(
        'ui-hello', '--minimum', '1', '--maximum', '1',
        '--client-id', "cli-smoke-$($run.RunId)"
    ) | ConvertFrom-Json
    if ($hello.schema_version -ne 1 -or
        -not $hello.accepted -or
        $hello.compatibility -ne 'compatible' -or
        $hello.client_id -ne "cli-smoke-$($run.RunId)" -or
        $hello.protocol_version -ne 1 -or
        $hello.server_pid -ne $runningProtocol.pid -or
        $hello.bootstrap_schema_version -ne 1 -or
        $hello.delta_schema_version -ne 1 -or
        @($hello.capabilities) -notcontains 'ordered_delta_poll') {
        throw 'ui-hello did not negotiate a stable renderer contract'
    }
    $tooNew = Invoke-AgenTerm @(
        'ui-hello', '--minimum', '2', '--maximum', '2',
        '--client-id', "future-renderer-$($run.RunId)"
    ) | ConvertFrom-Json
    if ($tooNew.accepted -or $tooNew.compatibility -ne 'client_too_new') {
        throw 'ui-hello did not report an incompatible future renderer explicitly'
    }
    $followTab = @($bootstrap.tabs | Where-Object title -eq $name)
    if ($followTab.Count -ne 1) {
        throw 'UI follow test could not resolve its stable tab'
    }
    $followNote = "ui-follow-$($run.RunId)"
    Invoke-AgenTerm @(
        'set-tab-note', '-t', $followTab[0].id, $followNote
    ) | Out-Null
    $deltas = Invoke-AgenTerm @(
        'ui-deltas',
        '--epoch', $hello.position.server_epoch,
        '--after', "$($hello.position.sequence)",
        '--limit', '64'
    ) | ConvertFrom-Json
    $previousSequence = [uint64]$hello.position.sequence
    foreach ($event in @($deltas.events)) {
        if ([uint64]$event.sequence -le $previousSequence) {
            throw 'ui-deltas returned duplicated or unordered events'
        }
        $previousSequence = [uint64]$event.sequence
    }
    $followEvent = @(
        $deltas.events |
            Where-Object {
                $_.kind -eq 'tab.note' -and
                $_.tab_id -eq $followTab[0].id
            } |
            Select-Object -Last 1
    )
    $followUpdate = @(
        $deltas.tab_updates |
            Where-Object id -eq $followTab[0].id
    )
    $postBootstrap = Invoke-AgenTerm @('ui-bootstrap') | ConvertFrom-Json
    $postTab = @(
        $postBootstrap.tabs |
            Where-Object id -eq $followTab[0].id
    )
    if ($deltas.schema_version -ne 1 -or
        $deltas.server_epoch -ne $hello.position.server_epoch -or
        $deltas.after_sequence -ne $hello.position.sequence -or
        $deltas.through_sequence -ne $deltas.current_sequence -or
        -not $deltas.complete -or $deltas.truncated -or
        $followEvent.Count -ne 1 -or
        $followUpdate.Count -ne 1 -or
        $followUpdate[0].note -ne $followNote -or
        $followUpdate[0].screen.generation -ne $deltas.current_sequence -or
        $postBootstrap.position.sequence -ne $deltas.current_sequence -or
        $postTab.Count -ne 1 -or
        $postTab[0].note -ne $followUpdate[0].note -or
        $postTab[0].screen.generation -ne $followUpdate[0].screen.generation) {
        throw 'ui-deltas did not return causal event and post-state identity'
    }
    $restartError = Invoke-AgenTermExpectedFailure @(
        'ui-deltas', '--epoch', 'definitely-stale-ui-epoch',
        '--after', '0'
    )
    $futureError = Invoke-AgenTermExpectedFailure @(
        'ui-deltas',
        '--epoch', $deltas.server_epoch,
        '--after', "$([uint64]$deltas.current_sequence + 1)"
    )
    if (-not $restartError.Contains('"code":"server_restart"') -or
        -not $restartError.Contains('"current"') -or
        -not $futureError.Contains('"code":"future_sequence"') -or
        -not $futureError.Contains('"current"')) {
        throw 'ui-deltas did not expose typed restart and future-position recovery'
    }
    Write-Evidence 'cli.ui-follow'

    $expectedOperations = @{
        'protocol.info' = 'observe'
        'ui.hello' = 'observe'
        'ui.bootstrap' = 'observe'
        'ui.deltas' = 'observe'
        'ui.tabs.show' = 'control'
        'ui.tabs.hide' = 'control'
        'ui.tabs.toggle' = 'control'
        'ui.tabs.set-width' = 'control'
        'tabs.set-note' = 'control'
        'server.kill' = 'destructive'
    }
    $operationIds = @($operationCatalog.operations.id)
    if (@($operationIds | Select-Object -Unique).Count -ne $operationIds.Count) {
        throw 'typed operation catalog contains duplicate stable IDs'
    }
    foreach ($operation in @($operationCatalog.operations)) {
        $propertyNames = @($operation.PSObject.Properties.Name)
        foreach ($requiredProperty in @(
            'id', 'class', 'command', 'aliases', 'parameters', 'result_type',
            'errors', 'events', 'destructive', 'available', 'since'
        )) {
            if ($propertyNames -notcontains $requiredProperty) {
                throw "typed operation $($operation.id) omitted $requiredProperty"
            }
        }
        if (-not $operation.available -or
            [string]::IsNullOrWhiteSpace($operation.result_type) -or
            [string]::IsNullOrWhiteSpace($operation.since) -or
            ($operation.class -eq 'destructive') -ne [bool]$operation.destructive) {
            throw "typed operation $($operation.id) exposed incomplete contract metadata"
        }
    }
    foreach ($entry in $expectedOperations.GetEnumerator()) {
        $operation = @(
            $operationCatalog.operations |
                Where-Object id -eq $entry.Key
        )
        if ($operation.Count -ne 1 -or $operation[0].class -ne $entry.Value) {
            throw "typed operation $($entry.Key) was absent or misclassified"
        }
    }
    $widthOperation = @(
        $operationCatalog.operations |
            Where-Object id -eq 'ui.tabs.set-width'
    )[0]
    $widthParameter = @($widthOperation.parameters)[0]
    if ($widthParameter.name -ne 'width' -or
        $widthParameter.value_type -ne 'integer' -or
        -not $widthParameter.required -or
        $widthParameter.minimum -ne 180 -or
        $widthParameter.maximum -ne 480 -or
        $widthOperation.result_type -ne 'ui_snapshot' -or
        @($widthOperation.events) -notcontains 'layout.tabs.width') {
        throw 'ui.tabs.set-width discovery did not expose its exact typed contract'
    }
    $toggleOperation = @(
        $operationCatalog.operations |
            Where-Object id -eq 'ui.tabs.toggle'
    )[0]
    if (@($toggleOperation.aliases) -notcontains 'toggle-tabs') {
        throw 'typed operation discovery omitted the legacy toggle-tabs alias'
    }
    $noteOperation = @(
        $operationCatalog.operations |
            Where-Object id -eq 'tabs.set-note'
    )[0]
    if ($noteOperation.script_surface -ne 'fleet.tabs.set_note' -or
        @($noteOperation.parameters).Count -ne 2 -or
        @($noteOperation.events) -notcontains 'tab.note') {
        throw 'tabs.set-note discovery did not expose its typed Fleet contract'
    }

    $tabsBaseline = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $configuredWidth = [int]$tabsBaseline.layout.sidebar.configured_width
    $hiddenTabs = Invoke-AgenTerm @('ui-action', 'tabs-hide') | ConvertFrom-Json
    if ($hiddenTabs.layout.sidebar.visible -or
        $hiddenTabs.layout.sidebar.effective_width -ne 0 -or
        $hiddenTabs.layout.sidebar.configured_width -ne $configuredWidth) {
        throw 'ui.tabs.hide did not preserve configured width while collapsing Tabs'
    }
    $tabsEvents = Invoke-AgenTerm @(
        'read-events',
        '--epoch', $tabsBaseline.event_position.epoch,
        '--after', "$($tabsBaseline.event_position.sequence)"
    ) | ConvertFrom-Json
    $hideEvent = @(
        $tabsEvents.events |
            Where-Object kind -eq 'layout.tabs.visibility' |
            Select-Object -Last 1
    )
    if ($hideEvent.Count -ne 1 -or
        $hideEvent[0].payload.operation_id -ne 'ui.tabs.hide') {
        throw 'Tabs visibility event was not attributed to ui.tabs.hide'
    }

    $shownTabs = Invoke-AgenTerm @('ui-action', 'tabs-show') | ConvertFrom-Json
    if (-not $shownTabs.layout.sidebar.visible) {
        throw 'ui.tabs.show did not reveal Tabs'
    }
    $toggledTabs = Invoke-AgenTerm @('ui-action', 'tabs-toggle') | ConvertFrom-Json
    if ($toggledTabs.layout.sidebar.visible) {
        throw 'ui.tabs.toggle did not invert Tabs visibility'
    }
    $legacyToggle = Invoke-AgenTerm @('ui-action', 'toggle-tabs') | ConvertFrom-Json
    if (-not $legacyToggle.layout.sidebar.visible) {
        throw 'legacy toggle-tabs did not map to ui.tabs.toggle'
    }

    $widthBaseline = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    foreach ($width in @(180, 480)) {
        $sizedTabs = Invoke-AgenTerm @(
            'ui-action', 'tabs-set-width', '--width', "$width"
        ) | ConvertFrom-Json
        if ($sizedTabs.layout.sidebar.configured_width -ne $width -or
            $sizedTabs.settings.tabs_width -ne $width) {
            throw "ui.tabs.set-width did not accept boundary $width"
        }
    }
    $widthEvents = Invoke-AgenTerm @(
        'read-events',
        '--epoch', $widthBaseline.event_position.epoch,
        '--after', "$($widthBaseline.event_position.sequence)"
    ) | ConvertFrom-Json
    $widthEvent = @(
        $widthEvents.events |
            Where-Object kind -eq 'layout.tabs.width' |
            Select-Object -Last 1
    )
    if ($widthEvent.Count -ne 1 -or
        $widthEvent[0].payload.operation_id -ne 'ui.tabs.set-width' -or
        $widthEvent[0].payload.configured_width -ne 480) {
        throw 'Tabs width event was not attributed to ui.tabs.set-width'
    }
    foreach ($width in @(179, 481)) {
        $widthError = Invoke-AgenTermExpectedFailure @(
            'ui-action', 'tabs-set-width', '--width', "$width"
        )
        if (-not $widthError.Contains(
            'operation_invalid_arguments[ui.tabs.set-width]'
        )) {
            throw "Tabs width $width did not fail with the stable typed error"
        }
    }
    $unknownTabsError = Invoke-AgenTermExpectedFailure @(
        'ui-action', 'tabs-teleport'
    )
    if (-not $unknownTabsError.Contains('operation_unknown[tabs-teleport]')) {
        throw 'unknown typed Tabs action did not fail with its stable error'
    }
    $afterInvalidTabs = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    if ($afterInvalidTabs.layout.sidebar.configured_width -ne 480 -or
        -not $afterInvalidTabs.layout.sidebar.visible) {
        throw 'invalid typed Tabs operations mutated committed layout state'
    }
    Invoke-AgenTerm @(
        'ui-action', 'tabs-set-width', '--width', "$configuredWidth"
    ) | Out-Null
    Write-Evidence 'cli.typed-tabs-operations'

    Write-Host 'STEP observable event snapshot, read, and bounded wait'
    $eventBaseline = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $eventName = "event-$PID"
    Invoke-AgenTerm @('new-window', '-d', '-n', $eventName) | Out-Null
    $eventBatch = Invoke-AgenTerm @(
        'read-events',
        '--epoch', $eventBaseline.event_position.epoch,
        '--after', "$($eventBaseline.event_position.sequence)"
    ) | ConvertFrom-Json
    $createdEvent = @(
        $eventBatch.events |
            Where-Object kind -eq 'tab.created' |
            Select-Object -First 1
    )
    if ($createdEvent.Count -ne 1 -or
        $createdEvent[0].sequence -le $eventBaseline.event_position.sequence -or
        $createdEvent[0].payload.index -lt 0) {
        throw 'read-events did not return the committed tab creation after the snapshot baseline'
    }
    $waitedEvent = Invoke-AgenTerm @(
        'wait-events',
        '--epoch', $eventBaseline.event_position.epoch,
        '--after', "$($eventBaseline.event_position.sequence)",
        '--kind', 'tab.created',
        '--tab', "@$($createdEvent[0].tab_id)",
        '--timeout-ms', '1000'
    ) | ConvertFrom-Json
    if ($waitedEvent.sequence -ne $createdEvent[0].sequence) {
        throw 'wait-events did not return the matching committed event'
    }
    $timeout = Invoke-AgenTermExpectedFailure @(
        'wait-events',
        '--epoch', $eventBatch.position.epoch,
        '--after', "$($eventBatch.position.sequence)",
        '--kind', 'never.test-event',
        '--timeout-ms', '20'
    )
    if (-not $timeout.Contains('event_wait_timeout')) {
        throw 'wait-events timeout did not return its stable typed error'
    }
    Write-Evidence 'cli.observable-events'

    Write-Host 'STEP composer round trip'
    Invoke-AgenTerm @('set-composer', '-t', $name, "echo $token") | Out-Null
    $beforeSubmit = Invoke-AgenTerm @('inspect', '-t', $name) | ConvertFrom-Json
    $draft = Invoke-AgenTerm @('show-composer', '-t', $name)
    if ($draft -ne "echo $token") {
        throw "Composer round trip mismatch: [$draft]"
    }

    Write-Host 'STEP submit and wait for output'
    $submitBaseline = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $submitRequestId = "submit-$($run.RunId)"
    $submitArgs = @(
        '--request-id', $submitRequestId, '--receipt-json',
        'send-composer', '-t', $name
    )
    $acceptedSubmit = Invoke-AgenTerm $submitArgs | ConvertFrom-Json
    if (
        $acceptedSubmit.outcome -ne 'accepted' -or
        $acceptedSubmit.wait.condition -ne 'submission_complete' -or
        $acceptedSubmit.wait.event_kind -ne 'composer.submission-finished'
    ) {
        throw 'send-composer did not expose an accepted receipt with a stable wait condition'
    }
    $pendingError = Invoke-AgenTermExpectedFailure @(
        'send-keys', '-t', $name, 'SHOULD_NOT_MERGE'
    )
    if (-not $pendingError.Contains('composer submission is pending')) {
        throw 'send-keys did not explicitly reject pending composer input'
    }
    Invoke-AgenTerm @(
        'wait-pane', '-t', $name, '--contains', $token,
        '--submit-complete', '--timeout-ms', '10000'
    ) | Out-Null
    $committedSubmit = Invoke-AgenTerm $submitArgs | ConvertFrom-Json
    if (
        $committedSubmit.outcome -ne 'committed' -or
        $committedSubmit.after_position.sequence -le
            $acceptedSubmit.after_position.sequence
    ) {
        throw 'send-composer replay did not advance from accepted to committed'
    }
    $submitEvents = Invoke-AgenTerm @(
        'read-events',
        '--epoch', $submitBaseline.event_position.epoch,
        '--after', "$($submitBaseline.event_position.sequence)"
    ) | ConvertFrom-Json
    $submitCompletion = @(
        $submitEvents.events |
            Where-Object {
                $_.kind -eq 'composer.submission-finished' -and
                $_.request_id -eq $submitRequestId -and
                $_.operation_id -eq $committedSubmit.operation_id
            }
    )
    if (
        $submitCompletion.Count -ne 1 -or
        -not $submitCompletion[0].payload.enter_written
    ) {
        throw 'composer completion event was not causally linked to its control request'
    }
    $afterSubmit = Invoke-AgenTerm @('inspect', '-t', $name) | ConvertFrom-Json
    if ($afterSubmit.windows[0].input_writes -ne
        ($beforeSubmit.windows[0].input_writes + 2) -or
        $afterSubmit.windows[0].submit_pending) {
        throw 'send-composer did not preserve separate text and Enter PTY events'
    }
    $capture = Invoke-AgenTerm @('capture-pane', '-p', '-t', $name)
    if (-not $capture.Contains($token)) {
        throw 'capture-pane did not contain the smoke token'
    }

    Write-Host 'STEP Backspace deletes exactly one input character'
    $backspacePrefix = "AGENTERM_BACKSPACE_$PID"
    Invoke-AgenTerm @(
        'send-keys', '-t', $name, '-l', "echo $backspacePrefix`X"
    ) | Out-Null
    Invoke-AgenTerm @('send-keys', '-t', $name, 'Backspace') | Out-Null
    Invoke-AgenTerm @('send-keys', '-t', $name, '-l', 'Y') | Out-Null
    Invoke-AgenTerm @('send-keys', '-t', $name, 'Enter') | Out-Null
    Invoke-AgenTerm @(
        'wait-pane', '-t', $name, '--contains', "$backspacePrefix`Y",
        '--timeout-ms', '10000'
    ) | Out-Null
    $backspaceCapture = Invoke-AgenTerm @('capture-pane', '-p', '-t', $name)
    if ($backspaceCapture -notmatch "(?m)^$([regex]::Escape($backspacePrefix))Y\s*$") {
        throw 'Backspace did not delete exactly the preceding input character'
    }
    Write-Evidence 'cli.backspace-del-one'

    Write-Host 'STEP terminal viewport scrolling'
    $scrollPrefix = "AGENTERM_SCROLL_$PID"
    Invoke-AgenTerm @(
        'set-composer', '-t', $name,
        "for /L %i in (1,1,80) do @echo $scrollPrefix`_%i"
    ) | Out-Null
    Invoke-AgenTerm @('send-composer', '-t', $name) | Out-Null
    Invoke-AgenTerm @(
        'wait-pane', '-t', $name, '--contains', "$scrollPrefix`_80",
        '--submit-complete', '--timeout-ms', '10000'
    ) | Out-Null
    $topOffset = [int](Invoke-AgenTerm @('scroll-pane', '-t', $name, 'top'))
    $topCapture = Invoke-AgenTerm @('capture-pane', '-p', '-t', $name)
    if ($topOffset -le 0 -or $topCapture.Contains("$scrollPrefix`_80")) {
        throw 'scroll-pane top did not move capture to historical terminal content'
    }
    $lowerOffset = [int](Invoke-AgenTerm @(
        'scroll-pane', '-t', $name, 'down', '5'
    ))
    if ($lowerOffset -ge $topOffset) {
        throw 'scroll-pane down did not reduce the scrollback offset'
    }
    $bottomOffset = [int](Invoke-AgenTerm @(
        'scroll-pane', '-t', $name, 'bottom'
    ))
    $bottomCapture = Invoke-AgenTerm @('capture-pane', '-p', '-t', $name)
    if ($bottomOffset -ne 0 -or
        -not $bottomCapture.Contains("$scrollPrefix`_80")) {
        throw 'scroll-pane bottom did not restore the live terminal viewport'
    }

    Write-Host 'STEP screenshots'
    Invoke-AgenTerm @('screenshot', '-o', $windowPng) | Out-Null
    Invoke-AgenTerm @('screenshot-pane', '-t', $name, '-o', $panePng) | Out-Null
    foreach ($image in @($windowPng, $panePng)) {
        if (-not (Test-Path -LiteralPath $image) -or (Get-Item $image).Length -lt 1000) {
            throw "Screenshot was not created correctly: $image"
        }
    }

    Write-Host 'STEP stable creation IDs survive unrelated index changes'
    $stableRootName = "stable-root-$PID"
    $stableChildName = "stable-child-$PID"
    $disposableName = "disposable-$PID"
    $stableRoot = Invoke-AgenTerm @(
        'new-window', '-d', '-n', $stableRootName, '-F', '#{window_id}'
    )
    if ($stableRoot -notmatch '^@\d+$') {
        throw "new-window stable format returned an invalid ID: $stableRoot"
    }
    $stableChild = Invoke-AgenTerm @(
        'new-window', '-d', '-n', $stableChildName,
        '--parent', $stableRoot, '-F', '#{window_id}'
    )
    $disposable = Invoke-AgenTerm @(
        'new-window', '-d', '-n', $disposableName, '-F', '#{window_id}',
        '--', 'cmd.exe', '/d', '/c', 'exit', '0'
    )
    Invoke-AgenTerm @(
        'wait-ui', '-t', $disposable, '--tab-state', 'dead', '--timeout-ms', '10000'
    ) | Out-Null
    Invoke-AgenTerm @('kill-window', '-t', $disposable) | Out-Null
    Invoke-AgenTerm @('select-window', '-t', $stableChild) | Out-Null
    Invoke-AgenTerm @(
        'wait-ui', '--active', $stableChild, '--timeout-ms', '10000'
    ) | Out-Null
    Invoke-AgenTerm @(
        'rename-window', '-t', $stableChild, "$stableChildName-renamed"
    ) | Out-Null
    $reportedParent = Invoke-AgenTerm @('show-tab-parent', '-t', $stableChild)
    if ($reportedParent -ne $stableRoot) {
        throw "stable child parent changed after unrelated close: $reportedParent"
    }
    Invoke-AgenTerm @('kill-window', '-t', $stableChild) | Out-Null
    Invoke-AgenTerm @('kill-window', '-t', $stableRoot) | Out-Null
    Write-Evidence 'cli.stable-create-id'

    Write-Host 'STEP remain on exit'
    Invoke-AgenTerm @('send-keys', '-t', $name, 'exit', 'Enter') | Out-Null
    Invoke-AgenTerm @('wait-pane', '-t', $name, '--dead', '--timeout-ms', '10000') | Out-Null
    Invoke-AgenTerm @(
        'wait-pane', '-t', $name, '--finalized', '--timeout-ms', '10000'
    ) | Out-Null
    $finalizedFirst = (
        Invoke-AgenTerm @('inspect', '-t', $name) | ConvertFrom-Json
    ).windows[0]
    $finalizedSecond = (
        Invoke-AgenTerm @('inspect', '-t', $name) | ConvertFrom-Json
    ).windows[0]
    if (
        -not $finalizedFirst.reader_closed -or
        -not $finalizedFirst.parser_drained -or
        -not $finalizedFirst.finalized -or
        $finalizedFirst.output_bytes -ne $finalizedSecond.output_bytes
    ) {
        throw 'Finalized terminal did not expose a stable drained output boundary'
    }
    $deadWrite = Invoke-AgenTermExpectedFailure @(
        '--receipt-json', 'send-keys', '-t', $name, '-l', 'late-input'
    ) | ConvertFrom-Json
    if (
        $deadWrite.outcome -ne 'no_op' -or
        $deadWrite.error.code -ne 'terminal_not_writable'
    ) {
        throw 'Finalized terminal write did not return a typed no-op receipt'
    }
    $state = Invoke-AgenTerm @(
        'display-message', '-p', '-t', $name,
        '#{window_id}:#{window_name}:#{pane_dead}'
    )
    if (-not $state.EndsWith(":${name}:1")) {
        throw "Exited tab did not remain visible and dead: $state"
    }
    Write-Evidence 'cli.remain-on-exit'

    Write-Host 'STEP explicit close'
    $closeBaseline = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $closingId = (
        $closeBaseline.tabs |
            Where-Object name -eq $name |
            Select-Object -First 1
    ).id
    Invoke-AgenTerm @('kill-window', '-t', $name) | Out-Null
    $closeEvents = Invoke-AgenTerm @(
        'read-events',
        '--epoch', $closeBaseline.event_position.epoch,
        '--after', "$($closeBaseline.event_position.sequence)"
    ) | ConvertFrom-Json
    $closeEvent = @(
        $closeEvents.events |
            Where-Object {
                $_.kind -eq 'tab.closed' -and $_.tab_id -eq
                    [uint64]$closingId.TrimStart('@')
            }
    )
    if (
        $closeEvent.Count -ne 1 -or
        -not $closeEvent[0].payload.terminal_shutdown_complete
    ) {
        throw 'explicit close did not prove bounded terminal worker shutdown'
    }
    $created = $false
    $runSucceeded = $true
    Write-Host "PASS: composer, viewport scroll, PTY I/O, waits, capture, PNG screenshots, remain-on-exit, manual close"
}
catch {
    $runFailure = $_
    throw
}
finally {
    Complete-SmokeRun -Context $run -Succeeded $runSucceeded `
        -FailureRecord $runFailure
}
