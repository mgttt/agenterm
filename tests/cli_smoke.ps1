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

$hadServer = $true
$savedErrorPreference = $ErrorActionPreference
$ErrorActionPreference = 'SilentlyContinue'
& $Exe list-sessions 2>$null | Out-Null
$probeExitCode = $LASTEXITCODE
$ErrorActionPreference = $savedErrorPreference
if ($probeExitCode -ne 0) {
    $hadServer = $false
}

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
    $draft = Invoke-AgenTerm @('show-composer', '-t', $name)
    if ($draft -ne "echo $token") {
        throw "Composer round trip mismatch: [$draft]"
    }

    Write-Host 'STEP submit and wait for output'
    Invoke-AgenTerm @('send-composer', '-t', $name) | Out-Null
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
    if (-not $hadServer) {
        & $Exe kill-server *> $null
    }
}
