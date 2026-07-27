param(
    [string]$Exe = (Join-Path $PSScriptRoot '..\dist\agenterm-cli.exe'),
    [switch]$ListEvidence
)

$ErrorActionPreference = 'Stop'
$declaredEvidence = @(
    'script.rhai-pure'
    'script.rhai-observe'
    'script.rhai-deny-budget'
    'script.rhai-framed'
    'script.supervisor'
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

function Add-FramePayload {
    param(
        [Parameter(Mandatory = $true)][IO.MemoryStream]$Stream,
        [Parameter(Mandatory = $true)][byte[]]$Payload
    )
    $length = [uint32]$Payload.Length
    $header = [byte[]]@(
        (($length -shr 24) -band 0xff)
        (($length -shr 16) -band 0xff)
        (($length -shr 8) -band 0xff)
        ($length -band 0xff)
    )
    $Stream.Write($header, 0, $header.Length)
    $Stream.Write($Payload, 0, $Payload.Length)
}

function Add-JsonFrame {
    param(
        [Parameter(Mandatory = $true)][IO.MemoryStream]$Stream,
        [Parameter(Mandatory = $true)][hashtable]$Frame
    )
    $json = $Frame | ConvertTo-Json -Compress -Depth 20
    Add-FramePayload -Stream $Stream -Payload ([Text.Encoding]::UTF8.GetBytes($json))
}

function New-FramedInvocation {
    param(
        [Parameter(Mandatory = $true)][string]$FrameId,
        [Parameter(Mandatory = $true)][string]$InvocationId,
        [Parameter(Mandatory = $true)][string]$Source,
        [uint32]$FrameVersion = 1
    )
    return @{
        frame_version = $FrameVersion
        frame_id = $FrameId
        kind = 'invoke'
        payload = @{
            envelope_version = 1
            invocation_id = $InvocationId
            api_version = 1
            operation = 'eval'
            profile = 'pure'
            source_label = 'smoke'
            source = $Source
            arguments = @()
            budgets = @{
                source_bytes = 262144
                operations = 1000000
                call_depth = 64
                expression_depth = 64
                collection_items = 10000
                string_bytes = 262144
                output_bytes = 65536
                wall_time_ms = 2000
            }
        }
    }
}

function Invoke-FramedWorker {
    param([Parameter(Mandatory = $true)][byte[]]$InputBytes)
    $startInfo = New-Object Diagnostics.ProcessStartInfo
    $startInfo.FileName = $workerExe
    $startInfo.Arguments = '--framed-worker'
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardInput = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($startInfo)
    try {
        $process.StandardInput.BaseStream.Write($InputBytes, 0, $InputBytes.Length)
        $process.StandardInput.Close()
        $output = New-Object IO.MemoryStream
        $process.StandardOutput.BaseStream.CopyTo($output)
        $stderr = $process.StandardError.ReadToEnd()
        $process.WaitForExit()
        if ($process.ExitCode -ne 0) {
            throw "framed worker exited $($process.ExitCode): $stderr"
        }
        return ,$output.ToArray()
    }
    finally {
        $process.Dispose()
    }
}

function ConvertFrom-FramedOutput {
    param([Parameter(Mandatory = $true)][byte[]]$Bytes)
    $frames = @()
    $offset = 0
    while ($offset -lt $Bytes.Length) {
        if ($Bytes.Length - $offset -lt 4) {
            throw 'framed worker returned a truncated length prefix'
        }
        $length = ([uint32]$Bytes[$offset] -shl 24) -bor
            ([uint32]$Bytes[$offset + 1] -shl 16) -bor
            ([uint32]$Bytes[$offset + 2] -shl 8) -bor
            [uint32]$Bytes[$offset + 3]
        $offset += 4
        if ($length -gt 2097152 -or $Bytes.Length - $offset -lt $length) {
            throw "framed worker returned an invalid $length byte payload"
        }
        $json = [Text.Encoding]::UTF8.GetString($Bytes, $offset, $length)
        $frames += $json | ConvertFrom-Json
        $offset += $length
    }
    return $frames
}

function Start-ScriptClient {
    param([Parameter(Mandatory = $true)][string]$Arguments)
    $startInfo = New-Object Diagnostics.ProcessStartInfo
    $startInfo.FileName = $Exe
    $startInfo.Arguments = $Arguments
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    return [Diagnostics.Process]::Start($startInfo)
}

function Wait-NewWorker {
    param(
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][int[]]$Existing,
        [int]$TimeoutMs = 3000
    )
    $deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMs)
    do {
        $worker = Get-Process -Name 'agenterm-script' -ErrorAction SilentlyContinue |
            Where-Object { $Existing -notcontains $_.Id } |
            Select-Object -First 1
        if ($null -ne $worker) {
            return $worker
        }
        Start-Sleep -Milliseconds 5
    } while ([DateTime]::UtcNow -lt $deadline)
    throw 'timed out waiting for a supervised agenterm-script worker'
}

function Wait-ProcessGone {
    param(
        [Parameter(Mandatory = $true)][int]$Id,
        [int]$TimeoutMs = 3000
    )
    $deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMs)
    do {
        if ($null -eq (Get-Process -Id $Id -ErrorAction SilentlyContinue)) {
            return
        }
        Start-Sleep -Milliseconds 10
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "process $Id remained alive after its supervisor exited"
}

if (-not ('AgenTermSmoke.NativeProcess' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
namespace AgenTermSmoke {
    public static class NativeProcess {
        [DllImport("kernel32.dll", SetLastError = true)]
        static extern IntPtr OpenProcess(uint access, bool inherit, int processId);
        [DllImport("kernel32.dll")]
        static extern bool CloseHandle(IntPtr handle);
        [DllImport("ntdll.dll")]
        static extern int NtSuspendProcess(IntPtr process);
        public static void Suspend(int processId) {
            IntPtr handle = OpenProcess(0x0800, false, processId);
            if (handle == IntPtr.Zero) {
                throw new System.ComponentModel.Win32Exception();
            }
            try {
                int status = NtSuspendProcess(handle);
                if (status != 0) throw new InvalidOperationException(
                    "NtSuspendProcess failed: 0x" + status.ToString("x"));
            } finally {
                CloseHandle(handle);
            }
        }
    }
}
'@
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
        $apiResult.value.framing.version -ne 1 -or
        $apiResult.value.framing.max_frame_bytes -ne 2097152 -or
        $apiResult.value.framing.input_kinds.broker_request -ne 'reserved' -or
        $apiResult.value.supervisor.job_object -ne 'kill_on_close' -or
        $apiResult.value.supervisor.global_concurrency -ne 4 -or
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

    Write-Host 'STEP framed worker sequencing, isolation, rejection, and recovery'
    $framedInput = New-Object IO.MemoryStream
    Add-FramePayload -Stream $framedInput -Payload ([Text.Encoding]::UTF8.GetBytes('{'))
    Add-FramePayload -Stream $framedInput -Payload ([byte[]]::new(2097153))
    Add-JsonFrame -Stream $framedInput -Frame (
        New-FramedInvocation -FrameId 'recovery' -InvocationId 'invoke-one' `
            -Source 'print("framed-output"); 40 + 2'
    )
    Add-JsonFrame -Stream $framedInput -Frame (
        New-FramedInvocation -FrameId 'frame-two' -InvocationId 'invoke-one' -Source '1'
    )
    Add-JsonFrame -Stream $framedInput -Frame (
        New-FramedInvocation -FrameId 'bad-version' -InvocationId 'never-run' `
            -Source '1' -FrameVersion 2
    )
    $framedOutput = Invoke-FramedWorker -InputBytes $framedInput.ToArray()
    $framedInput.Dispose()
    $frames = @(ConvertFrom-FramedOutput -Bytes $framedOutput)
    $recoveryFrame = @($frames | Where-Object { $_.frame_id -eq 'recovery' })
    $codes = @($frames | ForEach-Object { $_.payload.failure.code })
    if ($frames.Count -ne 5 -or
        $codes -notcontains 'protocol_malformed_frame' -or
        $codes -notcontains 'protocol_frame_too_large' -or
        $codes -notcontains 'protocol_duplicate_invocation' -or
        $codes -notcontains 'protocol_unsupported_frame_version' -or
        $recoveryFrame.Count -ne 1 -or
        -not $recoveryFrame[0].payload.ok -or
        $recoveryFrame[0].payload.stdout -ne "framed-output`n" -or
        $recoveryFrame[0].payload.value -ne 42) {
        throw 'framed worker did not preserve typed framing and recovery invariants'
    }
    $cancelInput = New-Object IO.MemoryStream
    $cancelInvocation = New-FramedInvocation -FrameId 'cancel-invoke' `
        -InvocationId 'cancel-running' -Source 'loop {}'
    $cancelInvocation.payload.budgets.wall_time_ms = 10000
    $cancelInvocation.payload.budgets.operations = 10000000
    Add-JsonFrame -Stream $cancelInput -Frame $cancelInvocation
    Add-JsonFrame -Stream $cancelInput -Frame @{
        frame_version = 1
        frame_id = 'cancel-request'
        kind = 'cancel'
        payload = @{ invocation_id = 'cancel-running' }
    }
    $cancelOutput = Invoke-FramedWorker -InputBytes $cancelInput.ToArray()
    $cancelInput.Dispose()
    $cancelFrames = @(ConvertFrom-FramedOutput -Bytes $cancelOutput)
    if ($cancelFrames.Count -ne 1 -or
        $cancelFrames[0].frame_id -ne 'cancel-invoke' -or
        $cancelFrames[0].payload.failure.code -ne 'limit_cancelled') {
        throw 'framed worker did not cooperatively cancel an active invocation'
    }
    Write-Evidence 'script.rhai-framed'

    Write-Host 'STEP supervisor timeout, crash, parent exit, concurrency, and recovery'
    $existingWorkers = @(
        Get-Process -Name 'agenterm-script' -ErrorAction SilentlyContinue |
            ForEach-Object { $_.Id }
    )
    $hardTimeoutClient = Start-ScriptClient -Arguments (
        'script eval "loop {}" --timeout-ms 500 --max-operations 10000000'
    )
    $hardTimeoutWorker = Wait-NewWorker -Existing $existingWorkers
    [AgenTermSmoke.NativeProcess]::Suspend($hardTimeoutWorker.Id)
    if (-not $hardTimeoutClient.WaitForExit(5000)) {
        $hardTimeoutClient.Kill()
        throw 'host deadline did not terminate a suspended worker'
    }
    $hardTimeoutError = $hardTimeoutClient.StandardError.ReadToEnd()
    if ($hardTimeoutClient.ExitCode -ne 3 -or
        -not $hardTimeoutError.Contains('"code":"host_hard_timeout"')) {
        throw "host hard timeout was not typed: $hardTimeoutError"
    }
    Wait-ProcessGone -Id $hardTimeoutWorker.Id
    $hardTimeoutClient.Dispose()

    $existingWorkers = @(
        Get-Process -Name 'agenterm-script' -ErrorAction SilentlyContinue |
            ForEach-Object { $_.Id }
    )
    $crashClient = Start-ScriptClient -Arguments (
        'script eval "loop {}" --timeout-ms 10000 --max-operations 10000000'
    )
    $crashWorker = Wait-NewWorker -Existing $existingWorkers
    $crashWorker.Kill()
    if (-not $crashClient.WaitForExit(5000)) {
        $crashClient.Kill()
        throw 'host did not finish after its worker crashed'
    }
    $crashError = $crashClient.StandardError.ReadToEnd()
    if ($crashClient.ExitCode -ne 1 -or
        -not $crashError.Contains('"code":"host_worker_crash"')) {
        throw "worker crash was not typed: $crashError"
    }
    $crashClient.Dispose()

    $existingWorkers = @(
        Get-Process -Name 'agenterm-script' -ErrorAction SilentlyContinue |
            ForEach-Object { $_.Id }
    )
    $interruptedClient = Start-ScriptClient -Arguments (
        'script eval "loop {}" --timeout-ms 10000 --max-operations 10000000'
    )
    $interruptedWorker = Wait-NewWorker -Existing $existingWorkers
    [AgenTermSmoke.NativeProcess]::Suspend($interruptedWorker.Id)
    $interruptedClient.Kill()
    $interruptedClient.WaitForExit()
    Wait-ProcessGone -Id $interruptedWorker.Id
    $interruptedClient.Dispose()

    $holderClients = @()
    $holderWorkers = @()
    for ($index = 0; $index -lt 4; $index++) {
        $existingWorkers = @(
            Get-Process -Name 'agenterm-script' -ErrorAction SilentlyContinue |
                ForEach-Object { $_.Id }
        )
        $holder = Start-ScriptClient -Arguments (
            'script eval "loop {}" --timeout-ms 10000 --max-operations 10000000'
        )
        $holderWorker = Wait-NewWorker -Existing $existingWorkers
        [AgenTermSmoke.NativeProcess]::Suspend($holderWorker.Id)
        $holderClients += $holder
        $holderWorkers += $holderWorker
    }
    $deniedClient = Start-ScriptClient -Arguments 'script eval "42"'
    if (-not $deniedClient.WaitForExit(3000)) {
        $deniedClient.Kill()
        throw 'global concurrency denial did not complete without spawning'
    }
    $deniedError = $deniedClient.StandardError.ReadToEnd()
    if ($deniedClient.ExitCode -ne 2 -or
        -not $deniedError.Contains('"code":"host_concurrency_limit"')) {
        throw "global concurrency ceiling was not typed: $deniedError"
    }
    $deniedClient.Dispose()

    $releasedWorkerId = $holderWorkers[0].Id
    $holderClients[0].Kill()
    $holderClients[0].WaitForExit()
    Wait-ProcessGone -Id $releasedWorkerId
    $holderClients[0].Dispose()
    $existingWorkers = @(
        Get-Process -Name 'agenterm-script' -ErrorAction SilentlyContinue |
            ForEach-Object { $_.Id }
    )
    $replacementClient = Start-ScriptClient -Arguments (
        'script eval "loop {}" --timeout-ms 10000 --max-operations 10000000'
    )
    $replacementWorker = Wait-NewWorker -Existing $existingWorkers
    [AgenTermSmoke.NativeProcess]::Suspend($replacementWorker.Id)

    for ($index = 1; $index -lt $holderClients.Count; $index++) {
        $workerId = $holderWorkers[$index].Id
        $holderClients[$index].Kill()
        $holderClients[$index].WaitForExit()
        Wait-ProcessGone -Id $workerId
        $holderClients[$index].Dispose()
    }
    $replacementClient.Kill()
    $replacementClient.WaitForExit()
    Wait-ProcessGone -Id $replacementWorker.Id
    $replacementClient.Dispose()
    $supervisorRecovered = Invoke-Script @('script', 'eval', '6 * 7')
    if ($supervisorRecovered -ne '42') {
        throw 'script supervisor did not recover after timeout/crash/interruption/concurrency'
    }
    Write-Evidence 'script.supervisor'

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
