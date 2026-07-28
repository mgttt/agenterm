param(
    [string]$CtlExe = (Join-Path $PSScriptRoot '..\dist\agenterm-cli.exe'),
    [string]$MuxExe = (Join-Path $PSScriptRoot '..\dist\agenterm-mux.exe'),
    [switch]$SkipEventLoad,
    [switch]$ListEvidence
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'TestHarness.ps1')
$declaredEvidence = @(
    'fleet.codex-launcher'
    'fleet.event-transition-catalog'
    'fleet.instance-discovery'
    'fleet.mux-frontend'
    'fleet.tab-environment'
    'fleet.upgrade-truth'
)
if ($ListEvidence) {
    $declaredEvidence
    exit 0
}

$CtlExe = [IO.Path]::GetFullPath($CtlExe)
$MuxExe = [IO.Path]::GetFullPath($MuxExe)
foreach ($path in @($CtlExe, $MuxExe)) {
    if (-not (Test-Path -LiteralPath $path)) {
        throw "AgenTerm executable not found: $path"
    }
}

$context = New-SmokeRunContext -Suite 'fleet' -Executable $CtlExe `
    -DeclaredEvidence $declaredEvidence
$address = $context.Address
$workspaceFile = $context.WorkspacePath
$instanceDir = $context.InstanceDirectory
$environmentName = "env-$PID"
$agentName = "codex-$PID"
$role = "reviewer-$PID"
$proxy = "http://127.0.0.1:$((30000 + ($PID % 1000)))"
$usedAddresses = [Collections.Generic.HashSet[string]]::new(
    [StringComparer]::Ordinal
)
$usedAddresses.Add($address) | Out-Null
function Get-FleetAddress {
    do {
        $candidate = Get-SmokeLoopbackAddress
    } while (-not $script:usedAddresses.Add($candidate))
    return $candidate
}
$explicitAddress = Get-FleetAddress
$secondAddress = Get-FleetAddress
Register-SmokeOwnedAddress -Context $context -Address $explicitAddress
Register-SmokeOwnedAddress -Context $context -Address $secondAddress
$explicitWorkspace = Join-Path $context.WorkspaceDirectory 'explicit.json'
$secondWorkspace = Join-Path $context.WorkspaceDirectory 'second.json'
$asyncDirectory = Join-Path $context.RunDirectory 'async'
New-Item -ItemType Directory -Path $asyncDirectory -Force | Out-Null
$secondStarted = $false
$eventWaiters = @()
$loadJobs = @()
$loadJobFiles = @()
$succeeded = $false
$failureRecord = $null

function Add-FleetCommandRecord {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string[]]$CommandArgs,
        [Parameter(Mandatory = $true)][int]$ExitCode,
        [Parameter(Mandatory = $true)][bool]$ExpectedFailure,
        [AllowNull()][string]$Output
    )

    $recordContext = $context.PSObject.Copy()
    $recordContext.Executable = $Path
    $recordContext.CommandLogBytes = $context.CommandLogBytes
    Add-SmokeCommandRecord -Context $recordContext -Arguments $CommandArgs `
        -ExitCode $ExitCode -ExpectedFailure $ExpectedFailure -Output $Output
    $context.CommandLogBytes = $recordContext.CommandLogBytes
}

function Write-Evidence {
    param([Parameter(Mandatory = $true)][string]$Id)
    Write-SmokeEvidence -Context $context -Id $Id
}

function Invoke-CheckedExe {
    param(
        [string]$Path,
        [string[]]$CommandArgs
    )
    $outputItems = @(& $Path @CommandArgs 2>&1)
    $exitCode = $LASTEXITCODE
    $output = $outputItems -join "`n"
    Add-FleetCommandRecord -Path $Path -CommandArgs $CommandArgs `
        -ExitCode $exitCode -ExpectedFailure $false -Output $output
    Sync-SmokeOwnedServers -Context $context
    if ($exitCode -ne 0) {
        throw "$([IO.Path]::GetFileName($Path)) $($CommandArgs -join ' ') failed:`n$($output -join "`n")"
    }
    return $output
}

function Invoke-ExpectedFailure {
    param(
        [string]$Path,
        [string[]]$CommandArgs
    )
    $savedErrorPreference = $ErrorActionPreference
    $ErrorActionPreference = 'SilentlyContinue'
    try {
        $outputItems = @(& $Path @CommandArgs 2>&1)
        $exitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $savedErrorPreference
    }
    $output = $outputItems -join "`n"
    Add-FleetCommandRecord -Path $Path -CommandArgs $CommandArgs `
        -ExitCode $exitCode -ExpectedFailure $true -Output $output
    if ($exitCode -eq 0) {
        throw "$([IO.Path]::GetFileName($Path)) $($CommandArgs -join ' ') unexpectedly succeeded"
    }
    return $output
}

function Start-EventWaiter {
    param(
        [string]$Epoch,
        [uint64]$After,
        [string]$Kind,
        [string]$Tag,
        [int]$TimeoutMs = 10000
    )
    $stdout = Join-Path $asyncDirectory "wait-$Tag.out"
    $stderr = Join-Path $asyncDirectory "wait-$Tag.err"
    $arguments = @(
        '--address', $address,
        'wait-events',
        '--epoch', $Epoch,
        '--after', "$After",
        '--kind', $Kind,
        '--timeout-ms', "$TimeoutMs"
    )
    $process = Start-Process -FilePath $CtlExe -ArgumentList $arguments `
        -RedirectStandardOutput $stdout -RedirectStandardError $stderr `
        -WindowStyle Hidden -PassThru
    Register-SmokeOwnedProcess -Context $context -Id $process.Id `
        -Kind 'client'
    $script:eventWaiters += $process
    return [pscustomobject]@{
        Process = $process
        Stdout = $stdout
        Stderr = $stderr
        Arguments = $arguments
    }
}

function Complete-EventWaiter {
    param(
        [Parameter(Mandatory = $true)]$Waiter,
        [int]$TimeoutMs = 15000
    )
    if (-not $Waiter.Process.WaitForExit($TimeoutMs)) {
        throw "event waiter $($Waiter.Process.Id) exceeded its bounded deadline"
    }
    $Waiter.Process.Refresh()
    $stdout = Get-Content -LiteralPath $Waiter.Stdout -Raw -ErrorAction SilentlyContinue
    $stderr = Get-Content -LiteralPath $Waiter.Stderr -Raw -ErrorAction SilentlyContinue
    Add-FleetCommandRecord -Path $CtlExe -CommandArgs $Waiter.Arguments `
        -ExitCode $Waiter.Process.ExitCode -ExpectedFailure $false `
        -Output "$stdout`n$stderr"
    if ($Waiter.Process.ExitCode -ne 0) {
        throw "event waiter failed with exit $($Waiter.Process.ExitCode): $stderr"
    }
    return ($stdout | ConvertFrom-Json)
}

try {
    $expectedVersion = ((Invoke-CheckedExe $CtlExe @('--version')) -split '\s+')[-1]
    Write-Host 'STEP control help and option errors are offline and fail fast'
    $topHelp = Invoke-CheckedExe $CtlExe @('--help')
    if (-not $topHelp.Contains('AgenTerm CLI')) {
        throw 'top-level --help did not render locally'
    }
    $catalog = Invoke-CheckedExe $CtlExe @('list-commands')
    foreach ($line in @($catalog -split "`n")) {
        if ($line -notmatch '^(\S+)(?: \(([^)]+)\))?$') {
            throw "could not parse command catalog line: $line"
        }
        foreach ($command in @($Matches[1], $Matches[2]) | Where-Object { $_ }) {
            $commandHelp = Invoke-CheckedExe $CtlExe @($command, '--help')
            if (-not $commandHelp.StartsWith('Usage: agenterm-cli ')) {
                throw "$command --help did not render command help offline"
            }
        }
    }
    $optionWatch = [Diagnostics.Stopwatch]::StartNew()
    $optionError = Invoke-ExpectedFailure $CtlExe @('-a', $address, 'list-windows')
    $optionWatch.Stop()
    if (-not $optionError.Contains('unknown global option') -or
        -not $optionError.Contains('--address HOST:PORT') -or
        $optionWatch.ElapsedMilliseconds -gt 2000) {
        throw 'mistyped -a did not fail fast with correct instance-targeting guidance'
    }

    Write-Host 'STEP zero-instance live control reports structured candidates'
    Remove-Item Env:AGENTERM_IPC_ADDRESS
    try {
        New-Item -ItemType Directory -Path $instanceDir -Force | Out-Null
        $deadRecord = Join-Path $instanceDir '4294967295.json'
        @{
            schema_version = 1
            pid = [uint32]::MaxValue
            address = '127.0.0.1:59999'
            version = $expectedVersion
            session = 'stale'
            workspace_path = 'stale-workspace.json'
            started_at_unix_ms = 1
        } | ConvertTo-Json | Set-Content -LiteralPath $deadRecord -Encoding utf8
        $emptyServers = Invoke-CheckedExe $CtlExe @('server-list', '--json') |
            ConvertFrom-Json
        if (@($emptyServers).Count -ne 0 -or
            (Test-Path -LiteralPath $deadRecord)) {
            throw (
                'server-list did not remove a definitively dead registration ' +
                'without autostarting a GUI'
            )
        }
        $emptyError = Invoke-ExpectedFailure $CtlExe @('list-windows')
        if (-not $emptyError.Contains('"code": "no_healthy_instance"') -or
            -not $emptyError.Contains('"candidates": []') -or
            -not $emptyError.Contains('list-instances --json')) {
            throw 'zero-instance selection error was not structured and actionable'
        }
    }
    finally {
        $env:AGENTERM_IPC_ADDRESS = $address
    }

    Write-Host 'STEP mux compatibility is discoverable without a server'
    $compatibility = Invoke-CheckedExe $MuxExe @('compatibility', '--json') |
        ConvertFrom-Json
    if ($compatibility.frontend -ne 'agenterm-mux' -or
        $compatibility.model.window -ne 'tab' -or
        @($compatibility.explicitly_unsupported.name) -notcontains 'split-window' -or
        @($compatibility.supported) -contains 'split-window') {
        throw 'agenterm-mux compatibility did not describe its honest single-pane model'
    }
    $muxCommands = Invoke-CheckedExe $MuxExe @('list-commands')
    if ($muxCommands -notmatch 'split-window \(unsupported:' -or
        $muxCommands -match '(?m)^split-window$') {
        throw 'agenterm-mux list-commands did not use the compatibility registry status'
    }
    $compatibilityText = Invoke-CheckedExe $MuxExe @('compatibility')
    $supportedLine = @($compatibilityText -split "`n") |
        Where-Object { $_ -like 'supported:*' } |
        Select-Object -First 1
    if ($supportedLine.Contains('split-window') -or
        $compatibilityText -notmatch 'unsupported: split-window') {
        throw 'Text compatibility output disagreed with the mux registry'
    }
    $splitError = Invoke-ExpectedFailure $MuxExe @('split-window')
    if (-not $splitError.Contains('unsupported')) {
        throw 'split-window dispatch did not report its registry status'
    }
    Invoke-ExpectedFailure $MuxExe @(
        '--address', '0.0.0.0:42000', 'compatibility'
    ) | Out-Null
    Write-Evidence 'fleet.mux-frontend'

    Write-Host 'STEP explicit CLI address autostarts the requested server'
    $savedAddress = $env:AGENTERM_IPC_ADDRESS
    $savedWorkspace = $env:AGENTERM_WORKSPACE_PATH
    Remove-Item Env:AGENTERM_IPC_ADDRESS
    $env:AGENTERM_WORKSPACE_PATH = $explicitWorkspace
    try {
        Invoke-CheckedExe $CtlExe @(
            '--address', $explicitAddress, 'new-window', '-d',
            '-n', "explicit-$PID", '--', 'cmd.exe', '/d', '/c', 'echo explicit'
        ) | Out-Null
        $explicitInstances = Invoke-CheckedExe $CtlExe @(
            'list-instances', '--json'
        ) | ConvertFrom-Json
        $explicitInstance = @($explicitInstances) |
            Where-Object address -eq $explicitAddress
        if ($null -eq $explicitInstance -or
            $explicitInstance.status -ne 'running' -or
            $explicitInstance.workspace_path -ne $explicitWorkspace) {
            throw 'agenterm-cli --address autostarted a different or undiscoverable server'
        }
    }
    finally {
        Write-Host 'STEP server-kill aliases the existing destructive operation'
        Invoke-CheckedExe $CtlExe @(
            '--address', $explicitAddress, 'server-kill'
        ) | Out-Null
        $env:AGENTERM_IPC_ADDRESS = $savedAddress
        $env:AGENTERM_WORKSPACE_PATH = $savedWorkspace
        Remove-Item -LiteralPath $explicitWorkspace -ErrorAction SilentlyContinue
    }

    Write-Host 'STEP tab-scoped environment and reserved AgenTerm context'
    Invoke-CheckedExe $CtlExe @(
        'new-window', '-d', '-n', $environmentName,
        '-e', "FLEET_ROLE=$role",
        '--', 'cmd.exe', '/d', '/c',
        'set FLEET_ROLE&set AGENTERM_TAB_ID&set AGENTERM_SESSION'
    ) | Out-Null
    Invoke-CheckedExe $CtlExe @(
        'wait-pane', '-t', $environmentName, '--dead', '--timeout-ms', '10000'
    ) | Out-Null

    Write-Host 'STEP live servers are discoverable and explicitly targetable'
    $instances = Invoke-CheckedExe $CtlExe @('list-instances', '--json') |
        ConvertFrom-Json
    $servers = Invoke-CheckedExe $CtlExe @('server-list', '--json') |
        ConvertFrom-Json
    $instance = @($instances) | Where-Object address -eq $address
    $server = @($servers) | Where-Object address -eq $address
    if ($null -eq $instance -or
        $null -eq $server -or
        $instance.status -ne 'running' -or
        $server.status -ne $instance.status -or
        $server.pid -ne $instance.pid -or
        $server.workspace_path -ne $instance.workspace_path -or
        $server.window_visible -ne $true -or
        $server.window_detached -ne $false -or
        $server.window_state -notin @('restored', 'maximized') -or
        $null -ne $server.modal_kind -or
        [string]::IsNullOrWhiteSpace($server.event_epoch) -or
        $null -eq $server.event_sequence -or
        $server.upgrade.status -ne 'same' -or
        $server.running_identity.git_commit -ne
            $server.staged_identity.git_commit -or
        $instance.version -ne $expectedVersion -or
        $instance.workspace_path -ne $workspaceFile) {
        throw (
            'list-instances/server-list did not report the same live server with ' +
            'typed window and event state'
        )
    }
    $targetedProtocol = Invoke-CheckedExe $CtlExe @(
        '--address', $address, 'protocol-info', '--running'
    ) | ConvertFrom-Json
    if (-not $targetedProtocol.features.instance_discovery -or
        $targetedProtocol.identity_scope -ne 'running_host' -or
        $targetedProtocol.pid -ne $server.pid -or
        -not $targetedProtocol.build_identity_complete -or
        $targetedProtocol.build_identity.git_commit -ne
            $server.running_identity.git_commit) {
        throw 'protocol-info --running did not report the requested server build'
    }
    $eventCatalog = @($targetedProtocol.event_catalog.events)
    $eventKinds = @($eventCatalog | ForEach-Object kind)
    if ($targetedProtocol.event_catalog.schema_version -ne 2 -or
        -not $targetedProtocol.features.typed_events -or
        $eventCatalog.Count -ne 26 -or
        @($eventKinds | Sort-Object -Unique).Count -ne $eventCatalog.Count -or
        @($eventCatalog | Where-Object {
                [string]::IsNullOrWhiteSpace($_.kind) -or
                [string]::IsNullOrWhiteSpace($_.state_path) -or
                [string]::IsNullOrWhiteSpace($_.payload) -or
                $_.scope -notin @('server', 'tab') -or
                [string]::IsNullOrWhiteSpace($_.since)
            }).Count -ne 0) {
        throw 'protocol-info event catalog was incomplete, open-ended, or untyped'
    }
    Write-Evidence 'fleet.upgrade-truth'

    Write-Host 'STEP server-scoped event transition matches its post-state'
    $layoutBaseline = Invoke-CheckedExe $CtlExe @('ui-snapshot') | ConvertFrom-Json
    $layoutBefore = [bool]$layoutBaseline.layout.sidebar.visible
    Invoke-CheckedExe $CtlExe @('ui-action', 'toggle-tabs') | Out-Null
    $layoutAfter = Invoke-CheckedExe $CtlExe @('ui-snapshot') | ConvertFrom-Json
    $layoutEvents = Invoke-CheckedExe $CtlExe @(
        'read-events',
        '--epoch', $layoutBaseline.event_position.epoch,
        '--after', "$($layoutBaseline.event_position.sequence)",
        '--limit', '32'
    ) | ConvertFrom-Json
    $visibilityEvent = @($layoutEvents.events |
        Where-Object kind -eq 'layout.tabs.visibility')[-1]
    if ($null -eq $visibilityEvent -or
        $visibilityEvent.tab_id -ne $null -or
        [bool]$visibilityEvent.payload.visible -ne [bool]$layoutAfter.layout.sidebar.visible -or
        [bool]$layoutAfter.layout.sidebar.visible -eq $layoutBefore) {
        throw 'server-scoped layout event did not describe its committed snapshot state'
    }
    Invoke-CheckedExe $CtlExe @('ui-action', 'toggle-tabs') | Out-Null
    Write-Evidence 'fleet.instance-discovery'

    $capture = Invoke-CheckedExe $CtlExe @('capture-pane', '-p', '-t', $environmentName)
    $snapshot = Invoke-CheckedExe $CtlExe @('ui-snapshot') | ConvertFrom-Json
    $tab = $snapshot.tabs | Where-Object name -eq $environmentName
    if ($capture -notmatch "FLEET_ROLE=$([regex]::Escape($role))" -or
        $capture -notmatch "AGENTERM_TAB_ID=$([regex]::Escape($tab.id))" -or
        $capture -notmatch 'AGENTERM_SESSION=agenterm' -or
        $tab.environment_names -notcontains 'FLEET_ROLE') {
        throw 'The child did not receive its scoped environment and authoritative tab context'
    }
    Write-Evidence 'fleet.tab-environment'

    Write-Host 'STEP snapshot-to-follow handoff is ordered for concurrent readers'
    $followBaseline = Invoke-CheckedExe $CtlExe @('ui-snapshot') | ConvertFrom-Json
    $readerA = Start-EventWaiter `
        $followBaseline.event_position.epoch `
        ([uint64]$followBaseline.event_position.sequence) `
        'tab.created' 'reader-a'
    $readerB = Start-EventWaiter `
        $followBaseline.event_position.epoch `
        ([uint64]$followBaseline.event_position.sequence) `
        'tab.created' 'reader-b'
    $followName = "follow-$PID"
    Invoke-CheckedExe $CtlExe @(
        'new-window', '-d', '-n', $followName,
        '--', 'cmd.exe', '/d', '/c', 'echo follow'
    ) | Out-Null
    $followA = Complete-EventWaiter $readerA
    $followB = Complete-EventWaiter $readerB
    if ($followA.kind -ne 'tab.created' -or
        $followA.sequence -le $followBaseline.event_position.sequence -or
        $followA.epoch -ne $followBaseline.event_position.epoch -or
        $followA.sequence -ne $followB.sequence -or
        $followA.tab_id -ne $followB.tab_id -or
        $followA.payload.index -ne $followB.payload.index) {
        throw 'concurrent waiters did not observe the same ordered event after the snapshot'
    }
    $followBatch = Invoke-CheckedExe $CtlExe @(
        'read-events',
        '--epoch', $followBaseline.event_position.epoch,
        '--after', "$($followBaseline.event_position.sequence)",
        '--limit', '1024'
    ) | ConvertFrom-Json
    $previousSequence = [uint64]$followBaseline.event_position.sequence
    $foundFollowEvent = $false
    foreach ($event in @($followBatch.events)) {
        if ([uint64]$event.sequence -le $previousSequence) {
            throw 'snapshot-to-follow batch was duplicated or out of order'
        }
        $previousSequence = [uint64]$event.sequence
        if ($event.sequence -eq $followA.sequence) {
            $foundFollowEvent = $true
        }
    }
    if (-not $foundFollowEvent) {
        throw 'snapshot-to-follow handoff omitted the event returned to both waiters'
    }
    $followSnapshot = Invoke-CheckedExe $CtlExe @('ui-snapshot') | ConvertFrom-Json
    $followTab = @($followSnapshot.tabs |
        Where-Object id -eq "@$($followA.tab_id)")[0]
    if ($null -eq $followTab -or
        $followTab.name -ne $followName -or
        $followTab.index -ne $followA.payload.index -or
        $followA.tab_id -eq $null) {
        throw 'tab-scoped create event did not describe its committed snapshot state'
    }
    Write-Evidence 'fleet.event-transition-catalog'

    Write-Host 'STEP timed out and cancelled waiters leave no client or server residue'
    $timeoutBaseline = Invoke-CheckedExe $CtlExe @('ui-snapshot') | ConvertFrom-Json
    $timeoutError = Invoke-ExpectedFailure $CtlExe @(
        'wait-events',
        '--epoch', $timeoutBaseline.event_position.epoch,
        '--after', "$($timeoutBaseline.event_position.sequence)",
        '--kind', 'never.fleet-timeout',
        '--timeout-ms', '25'
    )
    if (-not $timeoutError.Contains('"code":"event_wait_timeout"')) {
        throw 'bounded event wait did not report its typed timeout'
    }
    $serverBeforeCancel = @(
        (Invoke-CheckedExe $CtlExe @('server-list', '--json') | ConvertFrom-Json) |
            Where-Object address -eq $address
    )[0]
    $cancelWaiter = Start-EventWaiter `
        $timeoutBaseline.event_position.epoch `
        ([uint64]$timeoutBaseline.event_position.sequence) `
        'never.fleet-cancel' 'cancelled-reader' 60000
    if ($cancelWaiter.Process.HasExited) {
        throw 'long event waiter exited before cancellation could be exercised'
    }
    Stop-Process -Id $cancelWaiter.Process.Id -Force
    if (-not $cancelWaiter.Process.WaitForExit(5000)) {
        throw 'cancelled event waiter left a client process behind'
    }
    $serverAfterCancel = @(
        (Invoke-CheckedExe $CtlExe @('server-list', '--json') | ConvertFrom-Json) |
            Where-Object address -eq $address
    )[0]
    $afterCancel = Invoke-CheckedExe $CtlExe @(
        'read-events',
        '--epoch', $timeoutBaseline.event_position.epoch,
        '--after', "$($timeoutBaseline.event_position.sequence)"
    ) | ConvertFrom-Json
    if ($serverAfterCancel.pid -ne $serverBeforeCancel.pid -or
        $afterCancel.position.epoch -ne $timeoutBaseline.event_position.epoch) {
        throw 'timeout or cancellation disturbed the server-side observation stream'
    }

    Write-Host 'STEP one healthy instance is selected without an explicit address'
    Remove-Item Env:AGENTERM_IPC_ADDRESS
    try {
        $implicitWindows = Invoke-CheckedExe $CtlExe @(
            'list-windows', '-F', '#{window_name}'
        )
        if (-not $implicitWindows.Contains($environmentName)) {
            throw 'implicit single-instance selection targeted the wrong server'
        }
    }
    finally {
        $env:AGENTERM_IPC_ADDRESS = $address
    }

    Write-Host 'STEP multiple healthy instances require an explicit target'
    $savedWorkspace = $env:AGENTERM_WORKSPACE_PATH
    $env:AGENTERM_WORKSPACE_PATH = $secondWorkspace
    try {
        Invoke-CheckedExe $CtlExe @(
            '--address', $secondAddress, 'new-window', '-d',
            '-n', "second-$PID"
        ) | Out-Null
        $secondStarted = $true
    }
    finally {
        $env:AGENTERM_WORKSPACE_PATH = $savedWorkspace
    }
    Remove-Item Env:AGENTERM_IPC_ADDRESS
    try {
        $ambiguousError = Invoke-ExpectedFailure $CtlExe @('list-windows')
        if (-not $ambiguousError.Contains('"code": "ambiguous_instance"') -or
            -not $ambiguousError.Contains($address) -or
            -not $ambiguousError.Contains($secondAddress)) {
            throw 'multiple-instance selection error did not list both candidates'
        }
    }
    finally {
        $env:AGENTERM_IPC_ADDRESS = $address
    }
    $explicitWindows = Invoke-CheckedExe $CtlExe @(
        'list-windows', '-F', '#{window_name}'
    )
    if (-not $explicitWindows.Contains($environmentName) -or
        $explicitWindows.Contains("second-$PID")) {
        throw 'AGENTERM_IPC_ADDRESS did not take priority over discovery'
    }
    Invoke-CheckedExe $CtlExe @('--address', $secondAddress, 'shutdown') | Out-Null
    $secondStarted = $false

    Invoke-CheckedExe $CtlExe @(
        'new-session', '--', 'cmd.exe', '/d', '/c', 'echo child', '-s', 'hijack'
    ) | Out-Null
    $sessionAfterDelimiter = Invoke-CheckedExe $CtlExe @('list-sessions')
    if ($sessionAfterDelimiter -notmatch '^agenterm:') {
        throw 'A child -s argument after -- altered the live session'
    }

    Write-Host 'STEP supported Codex launcher proxy workflow'
    $agentCommand = (
        'if defined HTTPS_PROXY echo HTTPS_PROXY_PRESENT&' +
        'set NO_PROXY&set AGENTERM_TAB_ID'
    )
    Invoke-CheckedExe $CtlExe @(
        'new-agent', '-d', '-n', $agentName, '--program', 'cmd.exe',
        '--proxy', $proxy, '--no-proxy', 'localhost,127.0.0.1',
        '--', '/d', '/c', $agentCommand
    ) | Out-Null
    Invoke-CheckedExe $CtlExe @(
        'wait-pane', '-t', $agentName, '--dead', '--timeout-ms', '10000'
    ) | Out-Null
    $capture = Invoke-CheckedExe $CtlExe @('capture-pane', '-p', '-t', $agentName)
    $snapshot = Invoke-CheckedExe $CtlExe @('ui-snapshot') | ConvertFrom-Json
    $agent = $snapshot.tabs | Where-Object name -eq $agentName
    if (-not $capture.Contains('HTTPS_PROXY_PRESENT') -or
        -not $capture.Contains('NO_PROXY=localhost,127.0.0.1') -or
        $capture -notmatch "AGENTERM_TAB_ID=$([regex]::Escape($agent.id))" -or
        $capture.Contains('--dangerously-bypass-approvals-and-sandbox') -or
        $agent.environment_names -notcontains 'HTTPS_PROXY') {
        throw (
            "new-agent default was not safe or missed proxy/tab context`n" +
            "capture=$capture`n" +
            "agent=$($agent | ConvertTo-Json -Depth 6 -Compress)"
        )
    }
    Write-Evidence 'fleet.codex-launcher'

    Write-Host 'STEP child arguments cannot escape -- and --yolo is explicit'
    $delimiterName = "delimiter-$PID"
    Invoke-CheckedExe $CtlExe @(
        'new-agent', '-d', '-n', $delimiterName,
        '--program', 'cmd.exe', '--yolo', '--',
        '/d', '/c', 'echo %cmdcmdline%',
        '--parent', '@999999', '--program', 'missing-agent.exe',
        '--proxy', 'http://should-not-be-an-environment'
    ) | Out-Null
    Invoke-CheckedExe $CtlExe @(
        'wait-pane', '-t', $delimiterName, '--dead', '--timeout-ms', '10000'
    ) | Out-Null
    $delimiterCapture = Invoke-CheckedExe $CtlExe @(
        'capture-pane', '-p', '-t', $delimiterName
    )
    $delimiterInspect = Invoke-CheckedExe $CtlExe @(
        'inspect', '-t', $delimiterName
    ) | ConvertFrom-Json
    if (-not $delimiterCapture.Contains(
            '--dangerously-bypass-approvals-and-sandbox'
        ) -or
        $null -ne $delimiterInspect.windows[0].parent_id -or
        @($delimiterInspect.windows[0].environment_names) -contains 'HTTPS_PROXY') {
        throw '-- parsing or the explicit --yolo convenience mapping was incorrect'
    }
    Invoke-CheckedExe $CtlExe @('save-workspace') | Out-Null
    if ((Get-Content -LiteralPath $workspaceFile -Raw).Contains($proxy)) {
        throw 'Ephemeral proxy configuration leaked into persistent workspace state'
    }

    if (-not $SkipEventLoad) {
        Write-Host 'STEP bounded event history reports an explicit gap'
        $gapBaseline = Invoke-CheckedExe $CtlExe @('ui-snapshot') | ConvertFrom-Json
        $gapTarget = @($gapBaseline.tabs | Where-Object active)[0].id
        if (-not $gapTarget.StartsWith('@')) {
            throw 'event gap load could not resolve the active stable tab ID'
        }
        $workerCount = 16
        $eventsPerWorker = 258
        foreach ($worker in 0..($workerCount - 1)) {
            $jobOutputPath = Join-Path $asyncDirectory "load-$worker.log"
            $job = Start-Job -ScriptBlock {
                param($Executable, $ServerAddress, $Target, $Worker, $Count)
                for ($iteration = 0; $iteration -lt $Count; $iteration++) {
                    $workerOutput = @(
                        & $Executable --address $ServerAddress set-tab-note `
                            -t $Target "gap-$Worker-$iteration" 2>&1
                    )
                    if ($LASTEXITCODE -ne 0) {
                        throw (
                            "event load worker $Worker failed at iteration $iteration`: " +
                            ($workerOutput -join "`n")
                        )
                    }
                }
                "worker=$Worker events=$Count status=completed"
            } -ArgumentList @(
                $CtlExe, $address, $gapTarget, $worker, $eventsPerWorker
            )
            $loadJobs += $job
            $loadJobFiles += $jobOutputPath
        }
        $finishedLoad = @($loadJobs | Wait-Job -Timeout 180)
        if ($finishedLoad.Count -ne $workerCount) {
            throw 'bounded event load exceeded its job deadline'
        }
        for ($jobIndex = 0; $jobIndex -lt $loadJobs.Count; $jobIndex++) {
            $job = $loadJobs[$jobIndex]
            $jobOutput = @(Receive-Job -Job $job -ErrorAction Stop)
            $jobOutput -join "`n" |
                Set-Content -LiteralPath $loadJobFiles[$jobIndex] -Encoding UTF8
            if ($job.State -ne 'Completed') {
                throw "bounded event load job $($job.Id) ended in state $($job.State)"
            }
        }
        $gapError = Invoke-ExpectedFailure $CtlExe @(
            'read-events',
            '--epoch', $gapBaseline.event_position.epoch,
            '--after', "$($gapBaseline.event_position.sequence)"
        )
        if (-not $gapError.Contains('"code":"journal_gap"') -or
            -not $gapError.Contains('"earliest_available"') -or
            -not $gapError.Contains('"current"')) {
            throw 'bounded event history silently lost events instead of reporting journal_gap'
        }
    }
    else {
        Write-Host 'SKIP bounded event journal concurrent load'
    }

    Write-Host 'STEP a restarted server rejects positions from the previous epoch'
    $restartBefore = Invoke-CheckedExe $CtlExe @('ui-snapshot') | ConvertFrom-Json
    $serverBeforeRestart = @(
        (Invoke-CheckedExe $CtlExe @('server-list', '--json') | ConvertFrom-Json) |
            Where-Object address -eq $address
    )[0]
    Invoke-CheckedExe $CtlExe @('shutdown') | Out-Null
    $oldServer = Get-Process -Id $serverBeforeRestart.pid -ErrorAction SilentlyContinue
    if ($null -ne $oldServer) {
        try {
            Wait-Process -Id $serverBeforeRestart.pid -Timeout 10 -ErrorAction Stop
        }
        catch {
            if ($null -ne (
                    Get-Process -Id $serverBeforeRestart.pid `
                        -ErrorAction SilentlyContinue
                )) {
                throw
            }
        }
    }
    Invoke-CheckedExe $CtlExe @('start-server') | Out-Null
    Invoke-CheckedExe $CtlExe @(
        'wait-ui', '--window-state', 'restored', '--timeout-ms', '10000'
    ) | Out-Null
    $restartName = "restart-$PID"
    Invoke-CheckedExe $CtlExe @(
        'new-window', '-d', '-n', $restartName,
        '--', 'cmd.exe', '/d', '/c', 'echo restarted'
    ) | Out-Null
    $restartAfter = Invoke-CheckedExe $CtlExe @('ui-snapshot') | ConvertFrom-Json
    $serverAfterRestart = @(
        (Invoke-CheckedExe $CtlExe @('server-list', '--json') | ConvertFrom-Json) |
            Where-Object address -eq $address
    )[0]
    $restartError = Invoke-ExpectedFailure $CtlExe @(
        'read-events',
        '--epoch', $restartBefore.event_position.epoch,
        '--after', "$($restartBefore.event_position.sequence)"
    )
    if ($restartAfter.event_position.epoch -eq $restartBefore.event_position.epoch -or
        $serverAfterRestart.pid -eq $serverBeforeRestart.pid -or
        -not $restartError.Contains('"code":"server_restart"') -or
        -not $restartError.Contains('"requested_epoch"') -or
        -not $restartError.Contains('"current"')) {
        throw 'server restart did not reject the stale observation epoch with typed recovery data'
    }

    Write-Host 'STEP mux uses shared IPC, address override, and native namespace'
    $savedAddress = $env:AGENTERM_IPC_ADDRESS
    Remove-Item Env:AGENTERM_IPC_ADDRESS
    try {
        $windows = Invoke-CheckedExe $MuxExe @(
            '--address', $address, 'list-windows', '-F', '#{window_name}'
        )
        if (-not $windows.Contains($agentName)) {
            throw 'agenterm-mux did not observe AgenTerm tabs through --address'
        }
        $protocol = Invoke-CheckedExe $MuxExe @(
            '--address', $address, 'agenterm', 'protocol-info', '--running'
        ) | ConvertFrom-Json
        if (-not $protocol.features.mux_frontend) {
            throw 'The namespaced native control plane did not advertise mux support'
        }
    }
    finally {
        $env:AGENTERM_IPC_ADDRESS = $savedAddress
    }

    Write-Host 'STEP destructive mux failures preserve the live session and tabs'
    $beforeDestructive = Invoke-CheckedExe $MuxExe @(
        '--address', $address, 'list-windows', '-F', '#{window_id}:#{window_name}'
    )
    Invoke-ExpectedFailure $MuxExe @(
        '--address', $address, '--session', 'wrong-session', 'attach'
    ) | Out-Null
    Invoke-ExpectedFailure $MuxExe @(
        '--address', $address, 'kill-session', '-t', 'wrong-session'
    ) | Out-Null
    Invoke-ExpectedFailure $MuxExe @(
        '--address', $address, 'kill-window', '-t', '@999999'
    ) | Out-Null
    $afterFailures = Invoke-CheckedExe $MuxExe @(
        '--address', $address, 'list-windows', '-F', '#{window_id}:#{window_name}'
    )
    if ($afterFailures -ne $beforeDestructive) {
        throw 'A wrong session/window target changed the live server or tab fleet'
    }

    $victimName = "victim-$PID"
    Invoke-CheckedExe $MuxExe @(
        '--address', $address, 'new-window', '-d', '-n', $victimName
    ) | Out-Null
    Invoke-CheckedExe $MuxExe @(
        '--address', $address, 'kill-window', '-t', $victimName
    ) | Out-Null
    $afterKillWindow = Invoke-CheckedExe $MuxExe @(
        '--address', $address, 'list-windows', '-F', '#{window_name}'
    )
    if ($afterKillWindow.Contains($victimName) -or
        -not $afterKillWindow.Contains($agentName)) {
        throw 'Targeted kill-window did not remove only its requested tab'
    }
    Invoke-ExpectedFailure $MuxExe @('ui-snapshot') | Out-Null

    Write-Host 'STEP matching kill-session performs the requested destructive action'
    Invoke-CheckedExe $MuxExe @(
        '--address', $address, 'kill-session', '-t', 'agenterm'
    ) | Out-Null

    Write-Host 'PASS: fleet launch, delimiter safety, loopback IPC, and destructive mux behavior'
    $succeeded = $true
}
catch {
    $failureRecord = $_
}
finally {
    $cleanupFailure = $null
    foreach ($waiter in $eventWaiters) {
        if (-not $waiter.HasExited) {
            Stop-Process -Id $waiter.Id -Force -ErrorAction SilentlyContinue
            $waiter.WaitForExit(5000) | Out-Null
        }
    }
    foreach ($job in $loadJobs) {
        Stop-Job -Job $job -ErrorAction SilentlyContinue
        Remove-Job -Job $job -Force -ErrorAction SilentlyContinue
    }
    try {
        $cleanupAddresses = @($explicitAddress, $secondAddress)
        if ($succeeded) {
            $cleanupAddresses += $address
        }
        foreach ($ownedAddress in $cleanupAddresses) {
            & $CtlExe --address $ownedAddress kill-server 2>$null | Out-Null
        }
        if ($succeeded) {
            Remove-Item Env:AGENTERM_IPC_ADDRESS -ErrorAction SilentlyContinue
            & $CtlExe server-list --json 2>$null | Out-Null
            $remainingRegistrations = @(
                Get-ChildItem -LiteralPath $instanceDir -File -Filter '*.json' `
                    -ErrorAction SilentlyContinue
            )
            if ($remainingRegistrations.Count -ne 0) {
                throw (
                    'fleet cleanup left stale instance registrations: ' +
                    ($remainingRegistrations.Name -join ', ')
                )
            }
        }
    }
    catch {
        $cleanupFailure = $_
        $succeeded = $false
        if ($null -eq $failureRecord) {
            $failureRecord = $_
        }
    }
    finally {
        $env:AGENTERM_IPC_ADDRESS = $address
    }
    Complete-SmokeRun -Context $context -Succeeded $succeeded `
        -FailureRecord $failureRecord
    if ($null -ne $cleanupFailure) {
        throw $cleanupFailure
    }
    if (-not $succeeded) {
        throw $failureRecord
    }
}

exit 0
