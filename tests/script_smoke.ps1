param(
    [string]$Exe = (Join-Path $PSScriptRoot '..\dist\agenterm-cli.exe'),
    [switch]$ListEvidence
)

$ErrorActionPreference = 'Stop'
$declaredEvidence = @(
    'script.rhai-pure'
    'script.rhai-observe'
    'script.rhai-deny-budget'
)
if ($ListEvidence) {
    $declaredEvidence
    exit 0
}

function Write-Evidence {
    param([Parameter(Mandatory = $true)][string]$Id)
    if ($declaredEvidence -notcontains $Id) {
        throw "Script smoke emitted undeclared evidence ID: $Id"
    }
    Write-Host "EVIDENCE $Id"
}
$Exe = [IO.Path]::GetFullPath($Exe)
if (-not (Test-Path -LiteralPath $Exe)) {
    throw "AgenTerm executable not found: $Exe"
}
$workerExe = Join-Path ([IO.Path]::GetDirectoryName($Exe)) 'agenterm-script.exe'
if (-not (Test-Path -LiteralPath $workerExe)) {
    throw "AgenTerm script worker not found: $workerExe"
}

$previousAddress = $env:AGENTERM_IPC_ADDRESS
$previousWorkspace = $env:AGENTERM_WORKSPACE_PATH
$address = "127.0.0.1:$((51000 + ($PID % 1000)))"
$workspaceFile = Join-Path $env:TEMP "agenterm-script-$PID.json"
$sourceFile = Join-Path $env:TEMP "agenterm-script-$PID.rhai"
$serverStarted = $false

function Invoke-Script {
    param([string[]]$CommandArgs)
    $output = & $Exe @CommandArgs 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "agenterm $($CommandArgs -join ' ') failed:`n$($output -join "`n")"
    }
    return ($output -join "`n")
}

function Invoke-ScriptFailure {
    param(
        [int]$ExpectedExit,
        [string[]]$CommandArgs
    )
    $savedPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $output = & $Exe @CommandArgs 2>&1
        $exitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $savedPreference
    }
    if ($exitCode -ne $ExpectedExit) {
        throw (
            "agenterm $($CommandArgs -join ' ') exited $exitCode, " +
            "expected $ExpectedExit`n$($output -join "`n")"
        )
    }
    return ($output -join "`n")
}

function Invoke-WorkerFailure {
    param([Parameter(Mandatory = $true)][string]$InputText)
    $savedPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $output = $InputText | & $workerExe --worker 2>&1
        $exitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $savedPreference
    }
    if ($exitCode -ne 1) {
        throw "agenterm-script --worker exited $exitCode, expected 1"
    }
    return ($output -join "`n")
}

try {
    Remove-Item Env:AGENTERM_IPC_ADDRESS -ErrorAction SilentlyContinue

    Write-Host 'STEP offline scripting API discovery'
    $apiResult = Invoke-Script @('script', 'api', '--json') | ConvertFrom-Json
    if (-not $apiResult.ok -or
        $apiResult.value.api_version -ne 1 -or
        $apiResult.value.profiles.pure.variables -notcontains 'args' -or
        $apiResult.value.profiles.observe.variables -notcontains 'observe' -or
        $apiResult.value.limits.defaults.wall_time_ms -ne 2000 -or
        $apiResult.value.limits.hard_maximums.wall_time_ms -ne 10000 -or
        $apiResult.value.limits.invocation_bytes -ne 2097152 -or
        $apiResult.value.exit_classes.limit -ne 3 -or
        $apiResult.value.failure_categories -notcontains 'protocol' -or
        @($apiResult.value.apis | Where-Object {
            $_.name -eq 'new_tab' -and -not $_.available
        }).Count -ne 1 -or
        $apiResult.value.deferred_capabilities -notcontains 'control') {
        throw 'script api did not expose the versioned fail-closed capability catalog'
    }

    Write-Host 'STEP pure eval, file run, arguments, and check'
    $value = Invoke-Script @('script', 'eval', '40 + 2')
    if ($value -ne '42') {
        throw "pure eval returned unexpected value: $value"
    }
    [IO.File]::WriteAllText($sourceFile, 'print("hello"); args[0] + args[1]')
    $run = Invoke-Script @('script', 'run', $sourceFile, '--', 'safe', '-script')
    if ($run -ne "hello`nsafe-script") {
        throw "script run did not preserve stdout/value separation and argv: $run"
    }
    $check = Invoke-Script @('script', 'check', $sourceFile)
    if ($check -ne 'OK') {
        throw 'script check did not validate a well-formed source file'
    }
    [IO.File]::WriteAllText($sourceFile, 'let = ;')
    $parseError = Invoke-ScriptFailure 1 @('script', 'check', $sourceFile)
    if (-not $parseError.Contains('"code":"script_parse"')) {
        throw "script check did not return a stable parse-error class: $parseError"
    }
    [IO.File]::WriteAllText($sourceFile, 'new_tab()')
    $unavailableApi = Invoke-ScriptFailure 1 @('script', 'check', $sourceFile)
    if (-not $unavailableApi.Contains('"code":"script_api_unavailable"')) {
        throw 'script check accepted an unavailable control API'
    }
    [IO.File]::WriteAllText($sourceFile, 'made_up_api()')
    $unknownApi = Invoke-ScriptFailure 1 @('script', 'check', $sourceFile)
    if (-not $unknownApi.Contains('"code":"script_api_unknown"')) {
        throw 'script check accepted an API absent from the shipped catalog'
    }
    Write-Evidence 'script.rhai-pure'

    Write-Host 'STEP pure authority denial and operation budget'
    $denied = Invoke-ScriptFailure 1 @('script', 'eval', 'observe')
    if (-not $denied.Contains('"code":"script_runtime"')) {
        throw 'pure profile unexpectedly received observe authority'
    }
    $limited = Invoke-ScriptFailure 3 @(
        'script', 'eval', 'loop {}', '--max-operations', '1000'
    )
    if (-not $limited.Contains('"code":"limit_operations"')) {
        throw 'operation exhaustion did not return a typed limit result'
    }
    $wallLimited = Invoke-ScriptFailure 3 @(
        'script', 'eval', 'loop {}', '--timeout-ms', '1',
        '--max-operations', '10000000'
    )
    if (-not $wallLimited.Contains('"code":"limit_wall_time"') -or
        -not $wallLimited.Contains('"exit_class":"limit"')) {
        throw 'wall-time exhaustion did not return a typed limit result'
    }
    [IO.File]::WriteAllText($sourceFile, (' ' * 262145))
    $sourceLimited = Invoke-ScriptFailure 3 @('script', 'check', $sourceFile)
    if (-not $sourceLimited.Contains('script source exceeds the 262144 byte limit')) {
        throw 'source exhaustion did not return the public limit exit'
    }
    $malformed = Invoke-WorkerFailure -InputText '{'
    if (-not $malformed.Contains('"code":"protocol_invalid_invocation"') -or
        -not $malformed.Contains('"exit_class":"protocol"')) {
        throw 'malformed invocation did not return a typed protocol envelope'
    }
    $oversized = Invoke-WorkerFailure -InputText ('x' * 2097153)
    if (-not $oversized.Contains('"code":"protocol_invocation_too_large"')) {
        throw 'oversized invocation was not rejected at the worker protocol boundary'
    }
    $recovered = Invoke-Script @('script', 'eval', '6 * 7')
    if ($recovered -ne '42') {
        throw 'worker invocation did not recover after malformed protocol inputs'
    }
    Write-Evidence 'script.rhai-deny-budget'

    Write-Host 'STEP brokered observe snapshot'
    $env:AGENTERM_IPC_ADDRESS = $address
    $env:AGENTERM_WORKSPACE_PATH = $workspaceFile
    Invoke-Script @(
        '--address', $address, 'new-window', '-d', '-n', "script-observe-$PID"
    ) | Out-Null
    $serverStarted = $true
    $snapshot = Invoke-Script @('ui-snapshot') | ConvertFrom-Json
    $observed = Invoke-Script @(
        'script', 'eval', 'observe.event_position.sequence', '--profile', 'observe'
    )
    if ([uint64]$observed -lt [uint64]$snapshot.event_position.sequence) {
        throw 'observe script did not receive the brokered snapshot event position'
    }
    $mutationDenied = Invoke-ScriptFailure 1 @(
        'script', 'eval', 'new_tab()', '--profile', 'observe'
    )
    if (-not $mutationDenied.Contains('"code":"script_runtime"')) {
        throw 'observe profile unexpectedly exposed a mutation API'
    }
    Write-Evidence 'script.rhai-observe'

    Write-Host 'PASS: safe scripting API, pure/observe profiles, denial, and budgets'
}
finally {
    if ($serverStarted) {
        & $Exe --address $address shutdown 2>$null | Out-Null
    }
    Remove-Item -LiteralPath $sourceFile -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $workspaceFile -ErrorAction SilentlyContinue
    if ($null -eq $previousAddress) {
        Remove-Item Env:AGENTERM_IPC_ADDRESS -ErrorAction SilentlyContinue
    }
    else {
        $env:AGENTERM_IPC_ADDRESS = $previousAddress
    }
    if ($null -eq $previousWorkspace) {
        Remove-Item Env:AGENTERM_WORKSPACE_PATH -ErrorAction SilentlyContinue
    }
    else {
        $env:AGENTERM_WORKSPACE_PATH = $previousWorkspace
    }
}
