param(
    [string]$CtlExe = (Join-Path $PSScriptRoot '..\dist\agenterm-cli.exe'),
    [string]$MuxExe = (Join-Path $PSScriptRoot '..\dist\agenterm-mux.exe'),
    [switch]$ListEvidence
)

$ErrorActionPreference = 'Stop'
$declaredEvidence = @(
    'fleet.codex-launcher'
    'fleet.instance-discovery'
    'fleet.mux-frontend'
    'fleet.tab-environment'
)
if ($ListEvidence) {
    $declaredEvidence
    exit 0
}

function Write-Evidence {
    param([Parameter(Mandatory = $true)][string]$Id)
    if ($declaredEvidence -notcontains $Id) {
        throw "Fleet smoke emitted undeclared evidence ID: $Id"
    }
    Write-Host "EVIDENCE $Id"
}

$CtlExe = [IO.Path]::GetFullPath($CtlExe)
$MuxExe = [IO.Path]::GetFullPath($MuxExe)
foreach ($path in @($CtlExe, $MuxExe)) {
    if (-not (Test-Path -LiteralPath $path)) {
        throw "AgenTerm executable not found: $path"
    }
}

$previousAddress = $env:AGENTERM_IPC_ADDRESS
$previousWorkspace = $env:AGENTERM_WORKSPACE_PATH
$previousInstanceDir = $env:AGENTERM_INSTANCE_DIR
$address = "127.0.0.1:$((48000 + ($PID % 1000)))"
$workspaceFile = Join-Path $env:TEMP "agenterm-fleet-$PID.json"
$instanceDir = Join-Path $env:TEMP "agenterm-fleet-instances-$PID"
$env:AGENTERM_IPC_ADDRESS = $address
$env:AGENTERM_WORKSPACE_PATH = $workspaceFile
$env:AGENTERM_INSTANCE_DIR = $instanceDir
$environmentName = "env-$PID"
$agentName = "codex-$PID"
$role = "reviewer-$PID"
$proxy = "http://127.0.0.1:$((30000 + ($PID % 1000)))"
$secondAddress = "127.0.0.1:$((50000 + ($PID % 1000)))"
$secondWorkspace = Join-Path $env:TEMP "agenterm-second-$PID.json"
$secondStarted = $false

function Invoke-CheckedExe {
    param(
        [string]$Path,
        [string[]]$CommandArgs
    )
    $output = & $Path @CommandArgs 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "$([IO.Path]::GetFileName($Path)) $($CommandArgs -join ' ') failed:`n$($output -join "`n")"
    }
    return ($output -join "`n")
}

function Invoke-ExpectedFailure {
    param(
        [string]$Path,
        [string[]]$CommandArgs
    )
    $savedErrorPreference = $ErrorActionPreference
    $ErrorActionPreference = 'SilentlyContinue'
    try {
        $output = & $Path @CommandArgs 2>&1
        $exitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $savedErrorPreference
    }
    if ($exitCode -eq 0) {
        throw "$([IO.Path]::GetFileName($Path)) $($CommandArgs -join ' ') unexpectedly succeeded"
    }
    return ($output -join "`n")
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
        $emptyServers = Invoke-CheckedExe $CtlExe @('server-list', '--json') |
            ConvertFrom-Json
        if (@($emptyServers).Count -ne 0) {
            throw 'server-list did not report an empty fleet without autostarting a GUI'
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
    $explicitAddress = "127.0.0.1:$((49000 + ($PID % 1000)))"
    $explicitWorkspace = Join-Path $env:TEMP "agenterm-explicit-$PID.json"
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
        $instance.version -ne $expectedVersion -or
        $instance.workspace_path -ne $workspaceFile) {
        throw 'list-instances and server-list did not report the same isolated live server'
    }
    $targetedProtocol = Invoke-CheckedExe $CtlExe @(
        '--address', $address, 'protocol-info'
    ) | ConvertFrom-Json
    if (-not $targetedProtocol.features.instance_discovery) {
        throw 'agenterm-cli --address did not target the requested server'
    }
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
    Invoke-CheckedExe $CtlExe @(
        'new-agent', '-d', '-n', $agentName, '--program', 'cmd.exe',
        '--proxy', $proxy, '--no-proxy', 'localhost,127.0.0.1',
        '--', '/d', '/c',
        'set HTTPS_PROXY&set NO_PROXY&set AGENTERM_TAB_ID'
    ) | Out-Null
    Invoke-CheckedExe $CtlExe @(
        'wait-pane', '-t', $agentName, '--dead', '--timeout-ms', '10000'
    ) | Out-Null
    $capture = Invoke-CheckedExe $CtlExe @('capture-pane', '-p', '-t', $agentName)
    $snapshot = Invoke-CheckedExe $CtlExe @('ui-snapshot') | ConvertFrom-Json
    $agent = $snapshot.tabs | Where-Object name -eq $agentName
    if (-not $capture.Contains("HTTPS_PROXY=$proxy") -or
        -not $capture.Contains('NO_PROXY=localhost,127.0.0.1') -or
        $capture -notmatch "AGENTERM_TAB_ID=$([regex]::Escape($agent.id))" -or
        $capture.Contains('--dangerously-bypass-approvals-and-sandbox') -or
        $agent.environment_names -notcontains 'HTTPS_PROXY') {
        throw 'new-agent default was not safe or missed proxy/tab context'
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
            '--address', $address, 'agenterm', 'protocol-info'
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
}
finally {
    if ($secondStarted) {
        & $CtlExe --address $secondAddress shutdown 2>$null | Out-Null
    }
    & $CtlExe kill-server 2>$null | Out-Null
    Remove-Item -LiteralPath $workspaceFile -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $secondWorkspace -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $instanceDir -Recurse -ErrorAction SilentlyContinue
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
    if ($null -eq $previousInstanceDir) {
        Remove-Item Env:AGENTERM_INSTANCE_DIR -ErrorAction SilentlyContinue
    } else {
        $env:AGENTERM_INSTANCE_DIR = $previousInstanceDir
    }
    # A successful destructive-session test leaves no server for best-effort cleanup.
    $global:LASTEXITCODE = 0
}
