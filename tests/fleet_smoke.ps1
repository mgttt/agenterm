param(
    [string]$CtlExe = (Join-Path $PSScriptRoot '..\dist\agentermctl.exe'),
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
            throw 'agentermctl --address autostarted a different or undiscoverable server'
        }
    }
    finally {
        & $CtlExe --address $explicitAddress shutdown 2>$null | Out-Null
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
    $instance = @($instances) | Where-Object address -eq $address
    if ($null -eq $instance -or
        $instance.status -ne 'running' -or
        $instance.version -ne $expectedVersion -or
        $instance.workspace_path -ne $workspaceFile) {
        throw 'list-instances did not report the isolated live server'
    }
    $targetedProtocol = Invoke-CheckedExe $CtlExe @(
        '--address', $address, 'protocol-info'
    ) | ConvertFrom-Json
    if (-not $targetedProtocol.features.instance_discovery) {
        throw 'agentermctl --address did not target the requested server'
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
    & $CtlExe kill-server 2>$null | Out-Null
    Remove-Item -LiteralPath $workspaceFile -ErrorAction SilentlyContinue
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
