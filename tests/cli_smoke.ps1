param(
    [string]$Exe = (Join-Path $PSScriptRoot '..\dist\agentermctl.exe')
)

$ErrorActionPreference = 'Stop'
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
        'new-agent',
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
    $afterSubmit = Invoke-AgenTerm @('inspect', '-t', $name) | ConvertFrom-Json
    if ($afterSubmit.windows[0].input_writes -ne
        ($beforeSubmit.windows[0].input_writes + 2)) {
        throw 'send-composer did not preserve separate text and Enter PTY events'
    }
    Invoke-AgenTerm @('wait-pane', '-t', $name, '--contains', $token, '--timeout-ms', '10000') | Out-Null
    $capture = Invoke-AgenTerm @('capture-pane', '-p', '-t', $name)
    if (-not $capture.Contains($token)) {
        throw 'capture-pane did not contain the smoke token'
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

    Write-Host 'STEP explicit close'
    Invoke-AgenTerm @('kill-window', '-t', $name) | Out-Null
    $created = $false
    Write-Host "PASS: composer, PTY I/O, waits, capture, PNG screenshots, remain-on-exit, manual close"
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
