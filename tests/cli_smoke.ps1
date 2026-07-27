param(
    [string]$Exe = (Join-Path $PSScriptRoot '..\dist\agentermctl.exe'),
    [switch]$ListEvidence
)

$ErrorActionPreference = 'Stop'
$declaredEvidence = @(
    'cli.backspace-del-one'
    'cli.remain-on-exit'
)
if ($ListEvidence) {
    $declaredEvidence
    exit 0
}

function Write-Evidence {
    param([Parameter(Mandatory = $true)][string]$Id)
    if ($declaredEvidence -notcontains $Id) {
        throw "CLI smoke emitted undeclared evidence ID: $Id"
    }
    Write-Host "EVIDENCE $Id"
}

$Exe = [IO.Path]::GetFullPath($Exe)
if (-not (Test-Path -LiteralPath $Exe)) {
    throw "AgenTerm executable not found: $Exe"
}

function Invoke-AgenTerm {
    param([string[]]$CommandArgs)
    $output = & $Exe @CommandArgs 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "agenterm $($CommandArgs -join ' ') failed:`n$($output -join "`n")"
    }
    return ($output -join "`n")
}

function Invoke-AgenTermExpectedFailure {
    param([string[]]$CommandArgs)
    $savedErrorPreference = $ErrorActionPreference
    $ErrorActionPreference = 'SilentlyContinue'
    try {
        $output = & $Exe @CommandArgs 2>&1
        $exitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $savedErrorPreference
    }
    if ($exitCode -eq 0) {
        throw "agenterm $($CommandArgs -join ' ') unexpectedly succeeded"
    }
    return ($output -join "`n")
}

$previousAddress = $env:AGENTERM_IPC_ADDRESS
$previousWorkspace = $env:AGENTERM_WORKSPACE_PATH
$env:AGENTERM_IPC_ADDRESS = "127.0.0.1:$((45000 + ($PID % 1000)))"
$workspaceFile = Join-Path $env:TEMP "agenterm-cli-$PID.json"
$env:AGENTERM_WORKSPACE_PATH = $workspaceFile
$name = "agenterm-smoke-$PID"
$token = "AGENTERM_SMOKE_$PID"
$targetDir = Join-Path $PSScriptRoot '..\target\smoke'
$windowPng = Join-Path $targetDir "$name-window.png"
$panePng = Join-Path $targetDir "$name-pane.png"
$created = $false

try {
    Write-Host 'STEP create tab'
    Invoke-AgenTerm @('new-window', '-d', '-n', $name) | Out-Null
    $created = $true

    Write-Host 'STEP discover aligned and extended commands'
    $commands = Invoke-AgenTerm @('list-commands')
    foreach ($expected in @(
        'new-window (neww)',
        'list-windows (lsw)',
        'send-keys (send)',
        'scroll-pane',
        'new-agent',
        'list-instances',
        'wait-pane (expect-pane)',
        'set-composer',
        'ui-snapshot'
    )) {
        if (-not $commands.Contains($expected)) {
            throw "list-commands did not advertise: $expected"
        }
    }

    Write-Host 'STEP composer round trip'
    Invoke-AgenTerm @('set-composer', '-t', $name, "echo $token") | Out-Null
    $beforeSubmit = Invoke-AgenTerm @('inspect', '-t', $name) | ConvertFrom-Json
    $draft = Invoke-AgenTerm @('show-composer', '-t', $name)
    if ($draft -ne "echo $token") {
        throw "Composer round trip mismatch: [$draft]"
    }

    Write-Host 'STEP submit and wait for output'
    Invoke-AgenTerm @('send-composer', '-t', $name) | Out-Null
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

    Write-Host 'STEP remain on exit'
    Invoke-AgenTerm @('send-keys', '-t', $name, 'exit', 'Enter') | Out-Null
    Invoke-AgenTerm @('wait-pane', '-t', $name, '--dead', '--timeout-ms', '10000') | Out-Null
    $state = Invoke-AgenTerm @(
        'display-message', '-p', '-t', $name,
        '#{window_id}:#{window_name}:#{pane_dead}'
    )
    if (-not $state.EndsWith(":${name}:1")) {
        throw "Exited tab did not remain visible and dead: $state"
    }
    Write-Evidence 'cli.remain-on-exit'

    Write-Host 'STEP explicit close'
    Invoke-AgenTerm @('kill-window', '-t', $name) | Out-Null
    $created = $false
    Write-Host "PASS: composer, viewport scroll, PTY I/O, waits, capture, PNG screenshots, remain-on-exit, manual close"
}
finally {
    if ($created) {
        & $Exe kill-window -t $name *> $null
    }
    & $Exe kill-server *> $null
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
