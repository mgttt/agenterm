param(
    [string]$Exe = (Join-Path $PSScriptRoot '..\dist\agenterm-cli.exe'),
    [switch]$ListEvidence,
    [switch]$InternalFailureBundleProbe
)

$ErrorActionPreference = 'Stop'
$declaredEvidence = @(
    'script.rhai-runtime'
    'script.rhai-fleet'
    'script.api-tree'
    'script.fleet-v2'
    'script.fleet-tabs-set-note'
    'script.direct-entry'
    'script.north-star'
    'script.rhai-robustness-budget'
    'script.rhai-framed'
    'script.exit-classes'
    'script.typed-errors'
    'script.modules-tasks'
    'script.stream'
    'script.http'
    'script.fs-lifecycle'
    'script.runtime-lifecycle'
    'script.supervisor'
    'script.audit'
)
if ($ListEvidence) {
    $declaredEvidence
    exit 0
}

. (Join-Path $PSScriptRoot 'TestHarness.ps1')

function Write-Evidence {
    param([Parameter(Mandatory = $true)][string]$Id)
    Write-SmokeEvidence -Context $smokeRun -Id $Id
}
$Exe = [IO.Path]::GetFullPath($Exe)
if (-not (Test-Path -LiteralPath $Exe)) {
    throw "AgenTerm executable not found: $Exe"
}
$workerExe = Join-Path ([IO.Path]::GetDirectoryName($Exe)) 'agenterm-script.exe'
if (-not (Test-Path -LiteralPath $workerExe)) {
    throw "AgenTerm script worker not found: $workerExe"
}
$guiExe = Join-Path ([IO.Path]::GetDirectoryName($Exe)) 'agenterm.exe'
if (-not (Test-Path -LiteralPath $guiExe -PathType Leaf)) {
    throw "AgenTerm GUI executable not found: $guiExe"
}

$smokeRun = New-SmokeRunContext -Suite 'script' -Executable $Exe `
    -DeclaredEvidence $declaredEvidence
$Exe = $smokeRun.Executable
$previousAuditPath = $env:AGENTERM_SCRIPT_AUDIT_PATH
$previousAuditSecret = $env:AGENTERM_AUDIT_ENV_SECRET
$address = $smokeRun.Address
$workspaceFile = $smokeRun.WorkspacePath
$runtimeDirectory = Join-Path $smokeRun.RunDirectory 'script-runtime'
New-Item -ItemType Directory -Path $runtimeDirectory -Force | Out-Null
$sourceFile = Join-Path $runtimeDirectory 'source.rhai'
$auditFile = Join-Path $runtimeDirectory 'audit.jsonl'
$runSucceeded = $false
$runFailure = $null
$script:ownedScriptClients = [Collections.Generic.List[Diagnostics.Process]]::new()
$abandonedTempOwnerPids = [Collections.Generic.List[int]]::new()

function Invoke-Script {
    param([string[]]$CommandArgs)
    Invoke-SmokeCli -Context $smokeRun -Arguments $CommandArgs
}

function Invoke-DirectScript {
    param([string[]]$CommandArgs)
    $savedPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $outputItems = @(& $workerExe @CommandArgs 2>&1)
        $exitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $savedPreference
    }
    $output = $outputItems -join "`n"
    Add-SmokeCommandRecord -Context $smokeRun `
        -Arguments (@('[agenterm-script]') + $CommandArgs) `
        -ExitCode $exitCode -ExpectedFailure $false -Output $output
    if ($exitCode -ne 0) {
        $safeCommand = (
            ConvertTo-SmokeSafeArguments -Arguments $CommandArgs
        ) -join ' '
        throw "agenterm-script $safeCommand failed:`n$output"
    }
    return $output
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
    Add-SmokeCommandRecord -Context $smokeRun -Arguments $CommandArgs `
        -ExitCode $exitCode -ExpectedFailure $true -Output ($output -join "`n")
    if ($exitCode -ne $ExpectedExit) {
        $safeCommand = (
            ConvertTo-SmokeSafeArguments -Arguments $CommandArgs
        ) -join ' '
        throw (
            "agenterm $safeCommand exited $exitCode, " +
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
            envelope_version = 2
            invocation_id = $InvocationId
            api_version = 2
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
    Register-SmokeOwnedProcess -Context $smokeRun -Id $process.Id `
        -Kind 'script-worker'
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
    $process = [Diagnostics.Process]::Start($startInfo)
    $script:ownedScriptClients.Add($process)
    Register-SmokeOwnedProcess -Context $smokeRun -Id $process.Id `
        -Kind 'script-client'
    return $process
}

function Start-HttpFixture {
    param(
        [Parameter(Mandatory = $true)][string]$FixturePath,
        [Parameter(Mandatory = $true)][string]$ReadyPath,
        [Parameter(Mandatory = $true)][string]$LogPath,
        [Parameter(Mandatory = $true)][string]$StopPath
    )
    $hostExe = [Diagnostics.Process]::GetCurrentProcess().MainModule.FileName
    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $hostExe
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    foreach ($argument in @(
        '-NoProfile',
        '-NonInteractive',
        '-ExecutionPolicy',
        'Bypass',
        '-File',
        $FixturePath,
        '-ReadyPath',
        $ReadyPath,
        '-LogPath',
        $LogPath,
        '-StopPath',
        $StopPath,
        '-MaxRequests',
        '9'
    )) {
        $startInfo.ArgumentList.Add($argument)
    }
    $process = [Diagnostics.Process]::Start($startInfo)
    Register-SmokeOwnedProcess -Context $smokeRun -Id $process.Id `
        -Kind 'script-http-fixture'
    return $process
}

function Wait-HttpFixtureReady {
    param(
        [Parameter(Mandatory = $true)][Diagnostics.Process]$Process,
        [Parameter(Mandatory = $true)][string]$ReadyPath,
        [int]$TimeoutMs = 3000
    )
    $deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMs)
    while ([DateTime]::UtcNow -lt $deadline) {
        if (Test-Path -LiteralPath $ReadyPath) {
            return Get-Content -LiteralPath $ReadyPath -Raw | ConvertFrom-Json
        }
        if ($Process.HasExited) {
            throw "HTTP fixture exited before readiness with $($Process.ExitCode)"
        }
        Start-Sleep -Milliseconds 5
    }
    throw 'timed out waiting for the loopback HTTP fixture'
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
            Register-SmokeOwnedProcess -Context $smokeRun -Id $worker.Id `
                -Kind 'script-worker'
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

function Protect-ScriptSmokeDiagnosticText {
    param([AllowNull()][string]$Text)
    if ($null -eq $Text) {
        return $null
    }
    foreach ($secret in @(
        'AUDIT_STDOUT_SECRET',
        'AUDIT_ARG_SECRET',
        'AUDIT_SOURCE_SECRET',
        'AUDIT_ENV_SECRET',
        'HTTP_CREDENTIAL_SECRET',
        'PRIVATE_PATH_SECRET',
        'PROXY_CREDENTIAL_SECRET'
    )) {
        $Text = $Text.Replace($secret, '<redacted>')
    }
    return $Text
}

function Protect-ScriptSmokeCommandLog {
    if (-not (Test-Path -LiteralPath $smokeRun.CommandLogPath)) {
        return
    }
    $text = [IO.File]::ReadAllText($smokeRun.CommandLogPath)
    $text = Protect-ScriptSmokeDiagnosticText -Text $text
    [IO.File]::WriteAllText(
        $smokeRun.CommandLogPath,
        $text,
        [Text.UTF8Encoding]::new($false)
    )
}

try {
    Remove-Item Env:AGENTERM_IPC_ADDRESS -ErrorAction SilentlyContinue
    $env:AGENTERM_SCRIPT_AUDIT_PATH = $auditFile
    $env:AGENTERM_AUDIT_ENV_SECRET = 'AUDIT_ENV_SECRET'

    Write-Host 'STEP offline scripting API discovery'
    $apiResult = Invoke-Script @('script', 'api', '--json') | ConvertFrom-Json
    if (-not $apiResult.ok -or
        $apiResult.value.api_version -ne 2 -or
        $apiResult.value.schema_version -ne 3 -or
        $apiResult.value.default_profile -ne 'local' -or
        $apiResult.value.execution_model -ne 'unrestricted_local' -or
        $apiResult.value.comparison.schema_version -ne 1 -or
        $apiResult.value.comparison.reviewed_on -ne '2026-07-29' -or
        $apiResult.value.comparison.nodejs.reviewed_version -ne '26.5.0' -or
        $apiResult.value.comparison.bun.reviewed_version -ne '1.3.14' -or
        $apiResult.value.profiles.pure.variables -notcontains 'fleet' -or
        $apiResult.value.profiles.pure.ambient_authority -notcontains 'ordinary_local_program' -or
        $apiResult.value.profiles.observe.variables -notcontains 'fleet' -or
        $apiResult.value.profiles.observe.ambient_authority -notcontains 'ordinary_local_program' -or
        $apiResult.value.profiles.local.variables -notcontains 'fleet' -or
        $apiResult.value.profiles.local.status -ne 'shipped' -or
        $apiResult.value.limits.defaults.wall_time_ms -ne 2000 -or
        $apiResult.value.limits.hard_maximums.wall_time_ms -ne 120000 -or
        $apiResult.value.limits.invocation_bytes -ne 2097152 -or
        $apiResult.value.framing.version -ne 1 -or
        $apiResult.value.framing.max_frame_bytes -ne 2097152 -or
        $apiResult.value.framing.input_kinds.broker_request -ne 'available_worker_to_host' -or
        $apiResult.value.limits.defaults.broker_requests -ne 64 -or
        $apiResult.value.limits.hard_maximums.capture_bytes -ne 262144 -or
        $apiResult.value.supervisor.job_object -ne 'kill_on_close' -or
        $apiResult.value.supervisor.global_concurrency -ne 4 -or
        $apiResult.value.exit_classes.limit -ne 3 -or
        $apiResult.value.exit_classes.child -ne 4 -or
        $apiResult.value.exit_classes.cancelled -ne 5 -or
        $apiResult.value.exit_classes.fleet -ne 6 -or
        @($apiResult.value.typed_error.fields).Count -ne 8 -or
        $apiResult.value.typed_error.catchable_slices -notcontains
            'std.process.Output.require_success' -or
        $apiResult.value.failure_categories -notcontains 'protocol' -or
        @($apiResult.value.entries | Where-Object {
            $_.stable_id -eq 'fleet.tabs.new' -and $_.status -eq 'planned'
        }).Count -ne 1 -or
        @($apiResult.value.entries | Where-Object {
            $_.surface_path -eq 'fleet.workspace.info' -and
            $_.status -eq 'shipped' -and
            $_.catalog_path -eq 'workspace.info' -and
            $_.operation_id -eq 'workspace.info'
        }).Count -ne 1 -or
        @($apiResult.value.entries | Where-Object {
            $_.stable_id -eq 'std.fs.read-to-string' -and
            $_.surface_path -eq 'std::fs::read_to_string' -and
            $_.rust_path -eq 'std::fs::read_to_string' -and
            $_.rust_mapping -eq 'adapted' -and
            $_.status -eq 'shipped' -and
            $_.stability -eq 'stable' -and
            $_.designed_on -eq '2026-07-28' -and
            $_.profiles -contains 'local'
        }).Count -ne 1 -or
        @($apiResult.value.entries | Where-Object {
            $_.stable_id -eq 'std.process.command' -and
            $_.surface_path -eq 'std::process::command' -and
            $_.rust_path -eq 'std::process::Command::new' -and
            $_.status -eq 'shipped' -and
            $_.stability -eq 'stable' -and
            $_.profiles -contains 'local'
        }).Count -ne 1 -or
        @($apiResult.value.entries | Where-Object {
            $_.stable_id -eq 'std.process.command-start' -and
            $_.surface_path -eq 'Command.start' -and
            $_.rust_path -eq 'std::process::Command::spawn' -and
            $_.semantic_differences -contains
                'Command::spawn is exposed as start because spawn is Rhai-reserved' -and
            $_.status -eq 'shipped'
        }).Count -ne 1 -or
        @($apiResult.value.entries | Where-Object {
            $_.stable_id -eq 'rhai.task.race' -and
            $_.surface_path -eq 'rhai::task::race' -and
            $_.status -eq 'shipped' -and
            $_.profiles -contains 'local'
        }).Count -ne 1 -or
        $apiResult.value.limits.max_active_tasks -ne 64 -or
        $apiResult.value.limits.http.default_timeout_ms -ne 2000 -or
        $apiResult.value.limits.http.max_timeout_ms -ne 10000 -or
        $apiResult.value.limits.http.max_body_bytes -ne 262144 -or
        @($apiResult.value.entries | Where-Object {
            $_.stable_id -eq 'rhai.http.request' -and
            $_.surface_path -eq 'rhai::http::request' -and
            $_.authority -eq 'network' -and
            $_.status -eq 'shipped' -and
            $_.profiles -contains 'local'
        }).Count -ne 1 -or
        @($apiResult.value.entries | Where-Object {
            $_.stable_id -eq 'rhai.http.start' -and
            $_.surface_path -eq 'rhai::http::start' -and
            $_.execution -eq 'background_task' -and
            $_.status -eq 'shipped'
        }).Count -ne 1 -or
        @($apiResult.value.entries | Where-Object {
            $_.stable_id -eq 'std.env.get' -and
            $_.surface_path -eq 'std::env::get' -and
            $_.rust_path -eq 'std::env::var' -and
            $_.semantic_differences -contains
                'std::env::var is exposed as get because var is Rhai-reserved' -and
            $_.status -eq 'shipped'
        }).Count -ne 1) {
        throw 'script api did not expose the versioned fail-closed capability catalog'
    }
    $unreviewedComparisons = @(
        $apiResult.value.entries |
            Where-Object {
                $_.comparisons.nodejs.relationship -notin @(
                    'similar', 'agenterm_specific', 'deferred', 'not_applicable'
                ) -or
                $_.comparisons.bun.relationship -notin @(
                    'similar', 'agenterm_specific', 'deferred', 'not_applicable'
                ) -or
                $_.comparisons.nodejs.reviewed_on -ne '2026-07-29' -or
                $_.comparisons.bun.reviewed_on -ne '2026-07-29' -or
                [string]::IsNullOrWhiteSpace(
                    $_.comparisons.nodejs.semantic_note
                ) -or
                [string]::IsNullOrWhiteSpace(
                    $_.comparisons.bun.semantic_note
                )
            }
    )
    if ($unreviewedComparisons.Count -ne 0) {
        throw 'Script API entries did not carry reviewed Node.js/Bun comparisons'
    }
    $fleetEntries = @(
        $apiResult.value.entries |
            Where-Object { -not [string]::IsNullOrWhiteSpace($_.operation_id) }
    )
    if ($fleetEntries.Count -ne 18 -or
        @($fleetEntries.operation_id | Sort-Object -Unique).Count -ne 18 -or
        @($fleetEntries | Where-Object {
            $_.surface_path -ne $_.operation.script_surface -or
            $_.operation.id -ne $_.operation_id
        }).Count -ne 0) {
        throw 'Script API v2 did not map every typed operation exactly once'
    }
    $pipeInfo = [Diagnostics.ProcessStartInfo]::new()
    $pipeInfo.FileName = $workerExe
    $pipeInfo.UseShellExecute = $false
    $pipeInfo.CreateNoWindow = $true
    $pipeInfo.RedirectStandardOutput = $true
    $pipeInfo.RedirectStandardError = $true
    $pipeInfo.ArgumentList.Add('api')
    $pipeInfo.ArgumentList.Add('--json')
    $pipeProcess = [Diagnostics.Process]::Start($pipeInfo)
    Register-SmokeOwnedProcess -Context $smokeRun -Id $pipeProcess.Id `
        -Kind 'script-client'
    try {
        $pipelinePrefix = $pipeProcess.StandardOutput.ReadLine()
        $pipeProcess.StandardOutput.Dispose()
        $pipelineError = $pipeProcess.StandardError.ReadToEnd()
        $pipeProcess.WaitForExit()
        $pipelineExit = $pipeProcess.ExitCode
    }
    finally {
        $pipeProcess.Dispose()
    }
    if ($pipelineExit -ne 0 -or [string]::IsNullOrEmpty($pipelinePrefix)) {
        throw (
            'script stdout did not treat a downstream closed pipe as a ' +
            "normal early consumer exit: code=$pipelineExit error=$pipelineError"
        )
    }

    Write-Host 'STEP filtered scripting API object tree'
    $apiTree = Invoke-DirectScript @(
        'api', 'std::fs', '--status', 'shipped', '--tree'
    )
    if ($apiTree -notlike 'AgenTerm Script API v2*module=std.fs*status=shipped*' -or
        -not $apiTree.Contains('read-to-string  [shipped]') -or
        -not $apiTree.Contains('std::fs::read_to_string') -or
        -not $apiTree.Contains('Node.js~node:fs') -or
        -not $apiTree.Contains('Bun~Bun.file / Bun.write / node:fs') -or
        $apiTree.Contains('[planned]')) {
        throw "script api did not render the filtered std::fs object tree: $apiTree"
    }
    $plannedFleet = (
        Invoke-Script @(
            'script', 'api', 'fleet', '--status', 'planned', '--json'
        ) | ConvertFrom-Json
    )
    $plannedFleetEntries = @($plannedFleet.value.entries)
    if (-not $plannedFleet.ok -or
        $plannedFleet.value.view.module -ne 'fleet' -or
        $plannedFleet.value.view.status -ne 'planned' -or
        $plannedFleet.value.view.entry_count -ne $plannedFleetEntries.Count -or
        $plannedFleetEntries.Count -eq 0 -or
        @($plannedFleetEntries | Where-Object {
            $_.status -ne 'planned' -or -not $_.stable_id.StartsWith('fleet.')
        }).Count -ne 0) {
        throw 'script api JSON module/status view did not contain only planned Fleet entries'
    }
    $invalidApiStatus = Invoke-ScriptFailure 2 @(
        'script', 'api', '--status', 'experimental'
    )
    if (-not $invalidApiStatus.Contains('script_api_status_invalid')) {
        throw "script api did not reject an unknown status with a stable code: $invalidApiStatus"
    }
    Write-Evidence 'script.api-tree'

    Write-Host 'STEP Rhai repository dogfood contract check'
    $catalogFile = Join-Path $runtimeDirectory 'script-catalog.json'
    [IO.File]::WriteAllText(
        $catalogFile,
        ($apiResult.value | ConvertTo-Json -Depth 100),
        [Text.UTF8Encoding]::new($false)
    )
    $repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
    $contractScript = Join-Path $repositoryRoot 'scripts\rhai\verify-script-contract.rhai'
    $runtimeSpecification = Join-Path $repositoryRoot 'docs\agenterm-script-runtime.md'
    $dogfoodResult = Invoke-Script @(
        'script', 'run', $contractScript, '--profile', 'local',
        '--', $runtimeSpecification, $catalogFile
    )
    if ($dogfoodResult -notlike 'PASS: Rhai verified the Script API contract*') {
        throw "Rhai repository dogfood returned unexpected output: $dogfoodResult"
    }

    Write-Host 'STEP local environment, process, duration, and child lifecycle'
    $processSource = Join-Path $runtimeDirectory 'process.rhai'
    [IO.File]::WriteAllText(
        $processSource,
        @'
let command = std::process::command("cmd.exe");
command.args([
    "/d", "/v:on", "/s", "/c",
    "set /p LINE=&echo out:!LINE!:%AGENTERM_PROCESS_TEST%&echo cwd:%CD% 1>&2&exit /b 7"
]);
command.current_dir(args[0]);
command.env("AGENTERM_PROCESS_TEST", "argv-safe");
command.stdin_text("hello\n");
command.timeout(std::time::Duration::from_secs(2));
let output = command.output();
#{
    success: output.success,
    exit_code: output.exit_code,
    stdout: output.stdout_text(),
    stderr: output.stderr_text(),
    complete: output.complete,
    truncated: output.truncated,
    cwd: std::env::current_dir().display,
    has_path: std::env::has("PATH")
}
'@,
        [Text.UTF8Encoding]::new($false)
    )
    $processResult = Invoke-Script @(
        'script', 'run', $processSource, '--profile', 'local',
        '--timeout-ms', '10000', '--max-operations', '1000000',
        '--', $runtimeDirectory
    ) | ConvertFrom-Json
    if ($processResult.success -or
        $processResult.exit_code -ne 7 -or
        $processResult.stdout.Trim() -ne 'out:hello:argv-safe' -or
        $processResult.stderr -notlike "*cwd:$runtimeDirectory*" -or
        -not $processResult.complete -or
        $processResult.truncated -or
        -not $processResult.has_path -or
        [string]::IsNullOrWhiteSpace($processResult.cwd)) {
        throw 'local process output did not preserve argv, cwd, env, stdin, streams, and exit facts'
    }

    $timeoutExpression = @'
let command = std::process::command("cmd.exe");
command.args(["/d", "/s", "/c", "ping -n 6 127.0.0.1 >nul"]);
command.timeout(std::time::Duration::from_millis(10));
try {
    command.output();
    "missing-timeout"
} catch (error) {
    print(error);
}
'@
    $timeoutResult = Invoke-Script @(
        'script', 'eval', $timeoutExpression, '--profile', 'local',
        '--timeout-ms', '10000'
    )
    if ($timeoutResult -notlike '*process_timeout*') {
        throw "local process timeout was not typed: $timeoutResult"
    }
    $processRecovery = Invoke-Script @('script', 'eval', '6 * 7', '--profile', 'local')
    if ($processRecovery -ne '42') {
        throw 'script worker did not recover after a timed-out child process'
    }
    $childFailure = Invoke-ScriptFailure 4 @(
        'script', 'eval',
        'std::process::command("agenterm-definitely-missing.exe").output()',
        '--profile', 'local'
    )
    if (-not $childFailure.Contains('"code":"process_spawn"') -or
        -not $childFailure.Contains('"exit_class":"child"')) {
        throw 'unhandled child failure did not preserve its typed exit class'
    }
    $childNonzeroExpression = @'
let command = std::process::command("cmd.exe");
command.args(["/d", "/s", "/c", "exit /b 7"]);
let output = command.output();
output.require_success("test-child");
'@
    $childNonzero = Invoke-ScriptFailure 4 @(
        'script', 'eval', $childNonzeroExpression, '--profile', 'local'
    )
    if (-not $childNonzero.Contains('"code":"child_nonzero"') -or
        -not $childNonzero.Contains('"exit_class":"child"') -or
        -not $childNonzero.Contains('test-child')) {
        throw 'required nonzero child exit did not preserve its typed exit class'
    }
    $typedCatchExpression = @'
let caught = ();
try {
    let command = std::process::command("cmd.exe");
    command.args(["/d", "/s", "/c", "exit /b 7"]);
    let output = command.output();
    output.require_success("test-child");
} catch (error) {
    caught = error;
}
caught
'@
    $typedCatch = Invoke-Script @(
        'script', 'eval', $typedCatchExpression, '--profile', 'local', '--json'
    ) | ConvertFrom-Json
    if (-not $typedCatch.ok -or
        $typedCatch.value.class -ne 'child' -or
        $typedCatch.value.code -ne 'child_nonzero' -or
        $typedCatch.value.operation -ne 'std.process.Output.require_success' -or
        $typedCatch.value.safe_message -notlike 'test-child:*' -or
        $typedCatch.value.retryable -ne $false -or
        $typedCatch.value.target_kind -ne 'child_process' -or
        $typedCatch.value.truncated -ne $false -or
        $typedCatch.value.cause_class -ne 'exit_status') {
        throw 'Rhai catch did not receive the complete typed child error object'
    }
    Write-Evidence 'script.typed-errors'

    $childExpression = @'
let command = std::process::command("cmd.exe");
command.args(["/d", "/s", "/c", "ping -n 6 127.0.0.1 >nul"]);
command.timeout(std::time::Duration::from_secs(2));
let child = command.start();
let pid = child.id;
child.kill();
let output = child.wait_with_output();
#{ pid: pid, state: child.state, complete: output.complete }
'@
    $childResult = Invoke-Script @(
        'script', 'eval', $childExpression, '--profile', 'local',
        '--timeout-ms', '10000'
    ) | ConvertFrom-Json
    if ($childResult.pid -le 0 -or
        $childResult.state -ne 'exited' -or
        -not $childResult.complete) {
        throw 'local spawned child did not expose a truthful kill/wait lifecycle'
    }

    Write-Host 'STEP bounded child streams, backpressure, and truthful truncation'
    $streamFixtureExpected = (& $Exe --version).Trim()
    if ($LASTEXITCODE -ne 0 -or
        $streamFixtureExpected -notmatch (
            '^agenterm-cli \d+\.\d+\.\d+' +
            '(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$'
        )) {
        throw "offline stream fixture version failed: $streamFixtureExpected"
    }
    $streamFixtureLiteral = $Exe.Replace('\', '\\').Replace('"', '\"')
    $streamExpression = @'
let command = std::process::command("__STREAM_FIXTURE__");
command.arg("--version");
let child = command.start();
let stream = child.stdout;
let first = stream.read(2, std::time::Duration::from_secs(1)).to_text();
let rest = stream.collect(64, std::time::Duration::from_secs(1)).to_text();
let output = child.wait_with_output();
let empty = stream.read(1);
#{
    id: stream.id,
    kind: stream.kind,
    state: stream.state,
    buffered: stream.buffered_bytes,
    truncated: stream.truncated,
    complete: stream.complete,
    first: first,
    rest: rest,
    empty: empty.len,
    final: output.stdout_text(),
    output_complete: output.complete
}
'@.Replace('__STREAM_FIXTURE__', $streamFixtureLiteral)
    $streamResult = Invoke-Script @(
        'script', 'eval', $streamExpression, '--profile', 'local',
        '--timeout-ms', '10000'
    ) | ConvertFrom-Json
    if ($streamResult.id -le 0 -or
        $streamResult.kind -ne 'bytes' -or
        $streamResult.state -ne 'closed' -or
        $streamResult.buffered -ne 0 -or
        $streamResult.truncated -or
        -not $streamResult.complete -or
        $streamResult.first -ne 'ag' -or
        $streamResult.rest.Trim() -ne $streamFixtureExpected.Substring(2) -or
        $streamResult.empty -ne 0 -or
        $streamResult.final.Trim() -ne $streamFixtureExpected -or
        -not $streamResult.output_complete) {
        throw 'child stdout stream did not preserve bounded live and final output facts'
    }

    $truncatedExpression = @'
let command = std::process::command("__STREAM_FIXTURE__");
command.arg("--version");
command.capture_limit(4);
let child = command.start();
let stream = child.stdout;
let delivered = stream.collect(64).to_text();
let output = child.wait_with_output();
#{
    stdout: output.stdout_text(),
    truncated: output.truncated,
    complete: output.complete,
    delivered: delivered,
    stream_truncated: stream.truncated,
    stream_complete: stream.complete
}
'@.Replace('__STREAM_FIXTURE__', $streamFixtureLiteral)
    $truncatedResult = Invoke-Script @(
        'script', 'eval', $truncatedExpression, '--profile', 'local',
        '--timeout-ms', '10000'
    ) | ConvertFrom-Json
    if ($truncatedResult.stdout -ne 'agen' -or
        -not $truncatedResult.truncated -or
        $truncatedResult.complete -or
        $truncatedResult.delivered.Trim() -ne $streamFixtureExpected -or
        $truncatedResult.stream_truncated -or
        -not $truncatedResult.stream_complete) {
        throw 'live stream and bounded final capture conflated their completeness'
    }

    $streamCancelExpression = @'
let command = std::process::command("cmd.exe");
command.args(["/d", "/s", "/c", "ping -n 6 127.0.0.1 >nul"]);
command.timeout(std::time::Duration::from_secs(2));
let child = command.start();
let stream = child.stdout;
let timed_out = false;
try {
    stream.read(1, std::time::Duration::from_millis(10));
} catch (error) {
    timed_out = error.contains("stream_read_timeout");
}
let closed = stream.close();
child.kill();
let output = child.wait_with_output();
#{
    timed_out: timed_out,
    closed: closed,
    state: stream.state,
    truncated: stream.truncated,
    complete: stream.complete,
    output_truncated: output.truncated,
    output_complete: output.complete
}
'@
    $streamCancelResult = Invoke-Script @(
        'script', 'eval', $streamCancelExpression, '--profile', 'local',
        '--timeout-ms', '10000'
    ) | ConvertFrom-Json
    if (-not $streamCancelResult.timed_out -or
        -not $streamCancelResult.closed -or
        $streamCancelResult.state -ne 'cancelled' -or
        -not $streamCancelResult.truncated -or
        $streamCancelResult.complete -or
        -not $streamCancelResult.output_truncated -or
        $streamCancelResult.output_complete) {
        throw 'stream timeout and explicit close did not preserve cancellation facts'
    }
    Write-Evidence 'script.stream'

    Write-Host 'STEP task timers, deterministic race, cancellation, and wait timeout'
    $taskExpression = @'
let slow = rhai::task::after(std::time::Duration::from_millis(100));
let fast = rhai::task::after(std::time::Duration::from_millis(1));
let winner = rhai::task::race(
    [slow, fast],
    std::time::Duration::from_secs(1)
);
fast.wait();
let cancelled = slow.cancel();
#{
    winner: winner,
    fast_state: fast.state,
    fast_done: fast.done,
    slow_state: slow.state,
    slow_cancelled: slow.cancelled,
    cancel_changed_state: cancelled
}
'@
    $taskResult = Invoke-Script @(
        'script', 'eval', $taskExpression, '--profile', 'local',
        '--timeout-ms', '10000'
    ) | ConvertFrom-Json
    if ($taskResult.winner -ne 1 -or
        $taskResult.fast_state -ne 'completed' -or
        -not $taskResult.fast_done -or
        $taskResult.slow_state -ne 'cancelled' -or
        -not $taskResult.slow_cancelled -or
        -not $taskResult.cancel_changed_state) {
        throw 'task timer race/cancellation facts were not deterministic'
    }
    $taskTimeoutExpression = @'
let task = rhai::task::after(std::time::Duration::from_secs(1));
try {
    task.wait(std::time::Duration::from_millis(1));
    print("missing-timeout");
} catch (error) {
    print(error);
}
task.cancel();
'@
    $taskTimeout = Invoke-Script @(
        'script', 'eval', $taskTimeoutExpression, '--profile', 'local',
        '--timeout-ms', '10000'
    )
    if ($taskTimeout -notlike '*task_wait_timeout*') {
        throw "task wait timeout was not typed: $taskTimeout"
    }

    Write-Host 'STEP bounded loopback HTTP, async task, cancellation, and privacy'
    $httpFixtureScript = Join-Path $repositoryRoot 'tests\script_http_fixture.ps1'
    $httpReadyPath = Join-Path $runtimeDirectory 'http-ready.json'
    $httpLogPath = Join-Path $runtimeDirectory 'http-requests.jsonl'
    $httpStopPath = Join-Path $runtimeDirectory 'http-stop'
    $httpFixture = Start-HttpFixture -FixturePath $httpFixtureScript `
        -ReadyPath $httpReadyPath -LogPath $httpLogPath `
        -StopPath $httpStopPath
    try {
        $httpReady = Wait-HttpFixtureReady -Process $httpFixture `
            -ReadyPath $httpReadyPath
        if ($httpReady.schema_version -ne 1 -or
            $httpReady.pid -ne $httpFixture.Id -or
            $httpReady.url -notlike 'http://127.0.0.1:*' -or
            $httpReady.tls_url -notlike 'https://127.0.0.1:*') {
            throw 'loopback HTTP fixture returned invalid readiness facts'
        }

        $httpSource = Join-Path $runtimeDirectory 'http.rhai'
        [IO.File]::WriteAllText(
            $httpSource,
            @'
let base = args[0];
let tls_base = args[1];
let options = #{proxy: false, timeout: std::time::Duration::from_secs(1)};
let protocol_error_options = #{
    proxy: false,
    timeout: std::time::Duration::from_millis(500)
};

let status = rhai::http::request("GET", base + "/status", options);
let status_values = status.header("x-test");
let status_body = status.body;
let status_text = status_body.collect(16).to_text();

let echo = rhai::http::request("POST", base + "/echo", #{
    proxy: false,
    timeout: std::time::Duration::from_secs(2),
    headers: #{"content-type": "application/octet-stream"},
    body: rhai::bytes::from_text("payload")
});
let echo_text = echo.body.collect(16).to_text();

let large = rhai::http::request("GET", base + "/large", #{
    proxy: false,
    timeout: std::time::Duration::from_secs(2),
    max_body_bytes: 4
});
let large_body = large.body;
let large_text = large_body.collect(16).to_text();

let async_task = rhai::http::start("GET", base + "/async", options);
let async_kind = async_task.kind;
let async_response = async_task.wait(std::time::Duration::from_secs(2));
let async_text = async_response.body.collect(16).to_text();

let cancel_task = rhai::http::start("GET", base + "/cancel", options);
rhai::task::sleep(std::time::Duration::from_millis(50));
let cancel_changed = cancel_task.cancel();
let cancel_error = "";
try {
    cancel_task.wait(std::time::Duration::from_secs(1));
} catch (error) {
    cancel_error = error;
}
rhai::task::sleep(std::time::Duration::from_millis(600));
let cancel_state = cancel_task.state;

let timeout_error = "";
try {
    rhai::http::request("GET", base + "/slow", #{
        proxy: false,
        timeout: std::time::Duration::from_millis(50)
    });
} catch (error) {
    timeout_error = error;
}

let malformed_error = "";
try {
    rhai::http::request("GET", base + "/malformed", protocol_error_options);
} catch (error) {
    malformed_error = error;
}

let disconnect_error = "";
try {
    rhai::http::request("GET", base + "/disconnect", protocol_error_options);
} catch (error) {
    disconnect_error = error;
}

let tls_error = "";
try {
    rhai::http::request("GET", tls_base + "/tls", #{
        proxy: false,
        timeout: std::time::Duration::from_millis(250)
    });
} catch (error) {
    tls_error = error;
}

let proxy_error = "";
try {
    rhai::http::request(
        "GET",
        "http://HTTP_CREDENTIAL_SECRET.invalid/PRIVATE_PATH_SECRET",
        #{
            proxy: "http://user:PROXY_CREDENTIAL_SECRET@127.0.0.1:1",
            timeout: std::time::Duration::from_millis(50)
        }
    );
} catch (error) {
    proxy_error = error;
}

#{
    status: status.status,
    version: status.version,
    first_header: status_values[0].to_text(),
    second_header: status_values[1].to_text(),
    status_text: status_text,
    status_kind: status_body.kind,
    status_complete: status_body.complete,
    echo_text: echo_text,
    large_text: large_text,
    large_truncated: large_body.truncated,
    large_complete: large_body.complete,
    async_kind: async_kind,
    async_text: async_text,
    async_state: async_task.state,
    cancel_changed: cancel_changed,
    cancel_error: cancel_error,
    cancel_state: cancel_state,
    timeout_error: timeout_error,
    malformed_error: malformed_error,
    disconnect_error: disconnect_error,
    tls_error: tls_error,
    proxy_error: proxy_error
}
'@,
            [Text.UTF8Encoding]::new($false)
        )
        $httpResult = Invoke-Script @(
            'script', 'run', $httpSource, '--profile', 'local',
            '--timeout-ms', '10000', '--max-operations', '1000000',
            '--', $httpReady.url, $httpReady.tls_url
        ) | ConvertFrom-Json
        if ($httpResult.status -ne 201 -or
            $httpResult.version -ne 'HTTP/1.1' -or
            $httpResult.first_header -ne 'one' -or
            $httpResult.second_header -ne 'two' -or
            $httpResult.status_text -ne 'hello' -or
            $httpResult.status_kind -ne 'bytes' -or
            -not $httpResult.status_complete -or
            $httpResult.echo_text -ne 'payload' -or
            $httpResult.large_text -ne 'abcd' -or
            -not $httpResult.large_truncated -or
            $httpResult.large_complete -or
            $httpResult.async_kind -ne 'http' -or
            $httpResult.async_text -ne 'async-ok' -or
            $httpResult.async_state -ne 'completed' -or
            -not $httpResult.cancel_changed -or
            $httpResult.cancel_error -notlike '*task_cancelled*' -or
            $httpResult.cancel_state -ne 'cancelled' -or
            $httpResult.timeout_error -notlike '*http_timeout*' -or
            $httpResult.malformed_error -notlike '*http_*' -or
            $httpResult.disconnect_error -notlike '*http_transport*' -or
            $httpResult.tls_error -notlike '*http_tls*' -or
            $httpResult.proxy_error -notlike '*http_*') {
            throw 'HTTP client did not preserve typed response, stream, task, and error facts'
        }
        $httpJson = $httpResult | ConvertTo-Json -Compress
        foreach ($secret in @(
            'HTTP_CREDENTIAL_SECRET',
            'PRIVATE_PATH_SECRET',
            'PROXY_CREDENTIAL_SECRET'
        )) {
            if ($httpJson.Contains($secret)) {
                throw "HTTP diagnostics leaked secret sentinel: $secret"
            }
        }

        [IO.File]::WriteAllText($httpStopPath, 'stop')
        if (-not $httpFixture.WaitForExit(3000)) {
            throw 'loopback HTTP fixture did not exit after its bounded request set'
        }
        if ($httpFixture.ExitCode -ne 0) {
            throw "loopback HTTP fixture exited $($httpFixture.ExitCode)"
        }
        $httpRequests = @(
            Get-Content -LiteralPath $httpLogPath |
                ForEach-Object { $_ | ConvertFrom-Json }
        )
        $echoRequest = @($httpRequests | Where-Object path -eq '/echo')
        $tlsRequest = @($httpRequests | Where-Object tls -eq $true)
        $cancelRequest = @($httpRequests | Where-Object path -eq '/cancel')
        if ($httpRequests.Count -notin @(8, 9) -or
            $echoRequest.Count -ne 1 -or
            $echoRequest[0].method -ne 'POST' -or
            $echoRequest[0].body_bytes -ne 7 -or
            $tlsRequest.Count -ne 1 -or
            $cancelRequest.Count -gt 1) {
            throw 'loopback HTTP fixture did not observe the bounded request matrix'
        }
    }
    finally {
        if (-not $httpFixture.HasExited) {
            $httpFixture.Kill()
            $httpFixture.WaitForExit()
        }
        $httpFixture.Dispose()
    }
    $httpRecovery = Invoke-Script @(
        'script', 'eval', '6 * 7', '--profile', 'local'
    )
    if ($httpRecovery -ne '42') {
        throw 'script worker did not recover after HTTP timeout and cancellation'
    }
    Write-Evidence 'script.http'

    Write-Host 'STEP project modules and versioned named-task discovery'
    $projectFixture = Join-Path $repositoryRoot 'tests\fixtures\script-project'
    $taskManifest = Join-Path $projectFixture 'agenterm.tasks.json'
    $taskCatalog = Invoke-Script @(
        'script', 'task', 'list', '--manifest', $taskManifest, '--json'
    ) | ConvertFrom-Json
    $readyTask = @($taskCatalog.tasks | Where-Object id -eq 'daily-check')
    $degradedTask = @($taskCatalog.tasks | Where-Object id -eq 'missing-entry')
    $unknownFieldTask = @($taskCatalog.tasks | Where-Object id -eq 'unknown-field')
    $noExecuteTask = @($taskCatalog.tasks | Where-Object id -eq 'no-execute')
    if ($taskCatalog.schema_version -ne 2 -or
        [string]::IsNullOrWhiteSpace($taskCatalog.runtime_version) -or
        $taskCatalog.script_api_version -ne 2 -or
        $taskCatalog.script_catalog_schema_version -ne 3 -or
        $taskCatalog.project_id -ne 'script-smoke' -or
        $taskCatalog.origin.kind -ne 'repository' -or
        $taskCatalog.origin.id -ne 'agenterm' -or
        $taskCatalog.provenance.producer -ne 'agenterm-test' -or
        $taskCatalog.provenance.revision -ne 'fixture-1' -or
        -not $taskCatalog.compatible -or
        $null -ne $taskCatalog.compatibility_reason -or
        $taskCatalog.requirements.script_api.minimum -ne 2 -or
        $taskCatalog.requirements.script_api.maximum -ne 2 -or
        @($taskCatalog.requirements.capabilities).Count -ne 4 -or
        @($taskCatalog.requirements.capabilities) -notcontains
            'runtime.project.named-task' -or
        $readyTask.Count -ne 1 -or $readyTask[0].status -ne 'ready' -or
        $degradedTask.Count -ne 1 -or $degradedTask[0].status -ne 'degraded' -or
        $degradedTask[0].degraded_reason -notlike 'task_path_missing:*' -or
        $unknownFieldTask.Count -ne 1 -or
        $unknownFieldTask[0].status -ne 'degraded' -or
        $unknownFieldTask[0].degraded_reason -notlike 'task_manifest_entry:*' -or
        $noExecuteTask.Count -ne 1 -or $noExecuteTask[0].status -ne 'ready') {
        throw 'named-task discovery hid or misclassified a manifest entry'
    }
    $taskShow = Invoke-Script @(
        'script', 'task', 'show', 'daily-check',
        '--manifest', $taskManifest, '--json'
    ) | ConvertFrom-Json
    if ($taskShow.project_version -ne '1.0.0' -or
        $taskShow.script_api_version -ne 2 -or
        $taskShow.script_catalog_schema_version -ne 3 -or
        $taskShow.origin.id -ne 'agenterm' -or
        $taskShow.provenance.revision -ne 'fixture-1' -or
        -not $taskShow.compatible -or
        $taskShow.requirements.script_api.minimum -ne 2 -or
        @($taskShow.requirements.capabilities) -notcontains
            'runtime.project.module-import' -or
        @($taskShow.tasks).Count -ne 1 -or
        $taskShow.tasks[0].entry -ne 'main.rhai') {
        throw 'named-task inspection lost project or entry identity'
    }
    $taskCheck = Invoke-Script @(
        'script', 'task', 'check', 'daily-check',
        '--manifest', $taskManifest
    )
    if ($taskCheck -ne 'OK') {
        throw 'named-task check did not validate the compatible task without execution'
    }
    $noExecuteShow = Invoke-Script @(
        'script', 'task', 'show', 'no-execute',
        '--manifest', $taskManifest, '--json'
    ) | ConvertFrom-Json
    if ($noExecuteShow.tasks[0].entry -ne 'compile-only.rhai') {
        throw 'task show did not inspect the no-execution fixture'
    }
    $badVersion = Invoke-ScriptFailure 2 @(
        'script', 'task', 'list',
        '--manifest', (Join-Path $projectFixture 'bad-version.tasks.json')
    )
    if ($badVersion -notlike '*task_manifest_version:*') {
        throw 'task manifest version failure was not typed'
    }
    $incompatibleManifest = Join-Path $projectFixture 'incompatible.tasks.json'
    $incompatibleCatalog = Invoke-Script @(
        'script', 'task', 'list',
        '--manifest', $incompatibleManifest, '--json'
    ) | ConvertFrom-Json
    $incompatibleShow = Invoke-Script @(
        'script', 'task', 'show', 'must-not-run',
        '--manifest', $incompatibleManifest, '--json'
    ) | ConvertFrom-Json
    if ($incompatibleCatalog.compatible -or
        $incompatibleCatalog.compatibility_reason -ne
            'capability_unknown: future.capability' -or
        $incompatibleCatalog.requirements.script_api.minimum -ne 2 -or
        @($incompatibleCatalog.requirements.capabilities) -notcontains
            'future.capability' -or
        $incompatibleCatalog.tasks[0].status -ne 'ready' -or
        $incompatibleShow.compatible -or
        $incompatibleShow.compatibility_reason -ne
            'capability_unknown: future.capability' -or
        $incompatibleShow.tasks[0].entry -ne 'compile-only.rhai') {
        throw 'incompatible project requirements were not inspectable without execution'
    }
    $incompatibleCheck = Invoke-ScriptFailure 2 @(
        'script', 'task', 'check', 'must-not-run',
        '--manifest', $incompatibleManifest
    )
    $incompatibleRun = Invoke-ScriptFailure 2 @(
        'script', 'task', 'run', 'must-not-run',
        '--manifest', $incompatibleManifest
    )
    if ($incompatibleCheck -notlike
            '*task_project_incompatible: capability_unknown: future.capability*' -or
        $incompatibleRun -notlike
            '*task_project_incompatible: capability_unknown: future.capability*') {
        throw 'incompatible project requirements did not fail closed before execution'
    }
    $duplicateCatalog = Invoke-Script @(
        'script', 'task', 'list',
        '--manifest', (Join-Path $projectFixture 'duplicate.tasks.json'), '--json'
    ) | ConvertFrom-Json
    $duplicates = @($duplicateCatalog.tasks | Where-Object id -eq 'duplicate')
    if ($duplicates.Count -ne 2 -or
        @($duplicates | Where-Object status -eq 'degraded').Count -ne 1) {
        throw 'duplicate task identity was not kept visible and degraded'
    }

    $previousTaskEnvironment = [Environment]::GetEnvironmentVariable(
        'AGENTERM_TASK_FIXTURE',
        [EnvironmentVariableTarget]::Process
    )
    try {
        $env:AGENTERM_TASK_FIXTURE = 'fixture-ok'
        $namedTask = Invoke-Script @(
            'script', 'task', 'run', 'daily-check',
            '--manifest', $taskManifest, '--json', '--', 'cli-extra'
        ) | ConvertFrom-Json
        if (-not $namedTask.ok -or
            $namedTask.envelope_version -ne 2 -or
            $namedTask.value.answer -ne 42 -or
            $namedTask.value.environment -ne 'fixture-ok' -or
            @($namedTask.value.args).Count -ne 2 -or
            $namedTask.value.args[0] -ne 'manifest-default' -or
            $namedTask.value.args[1] -ne 'cli-extra' -or
            [IO.Path]::GetFileName($namedTask.value.cwd) -ne 'script-project') {
            throw 'named task did not preserve module, cwd, env-name, or argv contracts'
        }
    }
    finally {
        [Environment]::SetEnvironmentVariable(
            'AGENTERM_TASK_FIXTURE',
            $previousTaskEnvironment,
            [EnvironmentVariableTarget]::Process
        )
    }
    try {
        Remove-Item Env:AGENTERM_TASK_FIXTURE -ErrorAction SilentlyContinue
        $missingEnvironment = Invoke-ScriptFailure 2 @(
            'script', 'task', 'run', 'daily-check',
            '--manifest', $taskManifest
        )
    }
    finally {
        [Environment]::SetEnvironmentVariable(
            'AGENTERM_TASK_FIXTURE',
            $previousTaskEnvironment,
            [EnvironmentVariableTarget]::Process
        )
    }
    if ($missingEnvironment -notlike '*task_environment_missing:*') {
        throw 'named task did not reject a missing declared environment name'
    }
    $degradedRun = Invoke-ScriptFailure 2 @(
        'script', 'task', 'run', 'missing-entry',
        '--manifest', $taskManifest
    )
    if ($degradedRun -notlike '*task_degraded:*') {
        throw 'degraded named task did not fail with a typed configuration error'
    }

    $moduleCheck = Invoke-Script @(
        'script', 'check', (Join-Path $projectFixture 'main.rhai'),
        '--profile', 'local', '--project-root', $projectFixture
    )
    if ($moduleCheck -ne 'OK') {
        throw 'script check did not resolve a valid project-local module'
    }
    $noExecuteCheck = Invoke-Script @(
        'script', 'check', (Join-Path $projectFixture 'check-no-execute.rhai'),
        '--profile', 'local', '--project-root', $projectFixture
    )
    if ($noExecuteCheck -ne 'OK') {
        throw 'script check executed module top-level code instead of compiling only'
    }
    foreach ($failureFixture in @(
        @{ file = 'missing-module.rhai'; code = 'script_module_missing' }
        @{ file = 'escape-module.rhai'; code = 'script_module_root_escape' }
        @{ file = 'cycle-module.rhai'; code = 'script_module_cycle' }
        @{ file = 'bad-module-api.rhai'; code = 'script_api_unknown' }
    )) {
        $moduleFailure = Invoke-ScriptFailure 1 @(
            'script', 'check', (Join-Path $projectFixture $failureFixture.file),
            '--profile', 'local', '--project-root', $projectFixture
        )
        if ($moduleFailure -notlike "*$($failureFixture.code)*") {
            throw "module failure was not typed as $($failureFixture.code): $moduleFailure"
        }
    }
    Write-Evidence 'script.modules-tasks'

    Write-Host 'STEP Rhai Cargo target inventory migration'
    $targetReportScript = Join-Path $repositoryRoot 'scripts\rhai\target-report.rhai'
    $targetFixture = Join-Path $runtimeDirectory 'target'
    $targetDebugFixture = Join-Path $targetFixture 'debug'
    $targetDepsFixture = Join-Path $targetDebugFixture 'deps'
    New-Item -ItemType Directory -Path $targetDepsFixture -Force | Out-Null
    [IO.File]::WriteAllText(
        (Join-Path $targetFixture 'root.bin'),
        'x',
        [Text.UTF8Encoding]::new($false)
    )
    [IO.File]::WriteAllText(
        (Join-Path $targetDebugFixture 'debug.bin'),
        'abc',
        [Text.UTF8Encoding]::new($false)
    )
    [IO.File]::WriteAllText(
        (Join-Path $targetDepsFixture 'dependency.bin'),
        '12345',
        [Text.UTF8Encoding]::new($false)
    )
    $targetEnvelope = Invoke-Script @(
        'script', 'run', $targetReportScript, '--profile', 'local',
        '--timeout-ms', '10000', '--max-operations', '10000000',
        '--json', '--', $runtimeDirectory, 'target', '--json'
    ) | ConvertFrom-Json
    $targetReport = $targetEnvelope.stdout | ConvertFrom-Json
    $rootProfile = @($targetReport.profiles | Where-Object name -eq '(root)')
    $debugProfile = @($targetReport.profiles | Where-Object name -eq 'debug')
    if (-not $targetEnvelope.ok -or
        $targetReport.schema_version -ne 1 -or
        -not $targetReport.exists -or
        -not $targetReport.repo_local -or
        -not $targetReport.cleanup_allowed -or
        $targetReport.files -ne 3 -or
        $targetReport.bytes -ne 9 -or
        [string]::IsNullOrWhiteSpace($targetReport.oldest_write_utc) -or
        [string]::IsNullOrWhiteSpace($targetReport.newest_write_utc) -or
        $rootProfile.Count -ne 1 -or
        $rootProfile[0].files -ne 1 -or
        $rootProfile[0].bytes -ne 1 -or
        $debugProfile.Count -ne 1 -or
        $debugProfile[0].files -ne 2 -or
        $debugProfile[0].bytes -ne 8) {
        throw 'Rhai Cargo target inventory did not preserve the report contract'
    }

    Write-Host 'STEP pure eval, file run, arguments, and check'
    $value = Invoke-Script @('script', 'eval', '40 + 2')
    if ($value -ne '42') {
        throw "pure eval returned unexpected value: $value"
    }
    if ($InternalFailureBundleProbe) {
        throw "INTERNAL_FAILURE_BUNDLE_PROBE:script:$($smokeRun.RunId)"
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
    $localValue = Invoke-Script @(
        'script', 'eval', 'args[0] + args[1]', '--profile', 'local',
        '--', 'local-', 'profile'
    )
    if ($localValue -ne 'local-profile') {
        throw "explicit local profile returned unexpected value: $localValue"
    }
    $localDataFile = Join-Path $runtimeDirectory 'local-data.json'
    $localDataLiteral = $localDataFile.Replace('`', '``')
    $localRuntimeExpression = (
        "std::fs::write(``$localDataLiteral``, ``{`"answer`":42}``); " +
        "rhai::json::parse(std::fs::read_to_string(``$localDataLiteral``)).answer"
    )
    $localRuntime = Invoke-Script @(
        'script', 'eval', $localRuntimeExpression, '--profile', 'local'
    )
    if ($localRuntime -ne '42') {
        throw "local fs/json runtime returned unexpected value: $localRuntime"
    }
    $fsTree = Join-Path $runtimeDirectory 'fs-lifecycle'
    $fsNested = Join-Path $fsTree 'nested'
    $fsSource = Join-Path $fsNested 'source.txt'
    $fsCopy = Join-Path $fsNested 'copy.txt'
    $fsRenamed = Join-Path $fsTree 'renamed.txt'
    $fsTreeLiteral = $fsTree.Replace('`', '``')
    $fsNestedLiteral = $fsNested.Replace('`', '``')
    $fsSourceLiteral = $fsSource.Replace('`', '``')
    $fsCopyLiteral = $fsCopy.Replace('`', '``')
    $fsRenamedLiteral = $fsRenamed.Replace('`', '``')
    Invoke-Script @(
        'script', 'eval',
        "std::fs::create_dir_all(``$fsNestedLiteral``)",
        '--profile', 'local'
    ) | Out-Null
    Invoke-Script @(
        'script', 'eval',
        "std::fs::write(``$fsSourceLiteral``, ``Unicode lifecycle: 目录``)",
        '--profile', 'local'
    ) | Out-Null
    $copiedBytes = Invoke-Script @(
        'script', 'eval',
        "std::fs::copy(``$fsSourceLiteral``, ``$fsCopyLiteral``)",
        '--profile', 'local'
    )
    if ([int64]$copiedBytes -le 0) {
        throw "std::fs::copy returned an invalid byte count: $copiedBytes"
    }
    Invoke-Script @(
        'script', 'eval',
        "std::fs::rename(``$fsCopyLiteral``, ``$fsRenamedLiteral``)",
        '--profile', 'local'
    ) | Out-Null
    $renamedText = Invoke-Script @(
        'script', 'eval',
        "std::fs::read_to_string(``$fsRenamedLiteral``)",
        '--profile', 'local'
    )
    if ($renamedText -ne 'Unicode lifecycle: 目录') {
        throw "filesystem lifecycle corrupted Unicode content: $renamedText"
    }
    Invoke-Script @(
        'script', 'eval',
        "std::fs::remove_file(``$fsRenamedLiteral``)",
        '--profile', 'local'
    ) | Out-Null
    Invoke-Script @(
        'script', 'eval',
        "std::fs::remove_dir_all(``$fsTreeLiteral``)",
        '--profile', 'local'
    ) | Out-Null
    if (Test-Path -LiteralPath $fsTree) {
        throw 'filesystem lifecycle did not remove its exact owned tree'
    }
    $broadDelete = Invoke-ScriptFailure 1 @(
        'script', 'eval', 'std::fs::remove_dir_all(".")',
        '--profile', 'local'
    )
    if (-not $broadDelete.Contains('fs_remove_dir_all_broad_target')) {
        throw "broad filesystem cleanup did not return its stable error: $broadDelete"
    }
    if (-not (Test-Path -LiteralPath $runtimeDirectory)) {
        throw 'broad filesystem cleanup damaged the smoke-run workspace'
    }
    if ((Invoke-Script @(
        'script', 'eval', 'std::fs::exists(".")', '--profile', 'local'
    )) -ne 'true') {
        throw 'runtime did not recover after a rejected broad filesystem cleanup'
    }
    Write-Evidence 'script.fs-lifecycle'
    $ownedTempRoot = Invoke-Script @(
        'script', 'eval', 'rhai::runtime::temp_dir().display',
        '--profile', 'local'
    )
    if ([string]::IsNullOrWhiteSpace($ownedTempRoot) -or
        (Test-Path -LiteralPath $ownedTempRoot)) {
        throw "invocation-owned temporary root survived completion: $ownedTempRoot"
    }
    $atomicResult = Join-Path $runtimeDirectory 'atomic-result-目录.json'
    [IO.File]::WriteAllText($atomicResult, '{"state":"old"}')
    $atomicResultLiteral = $atomicResult.Replace('`', '``')
    $atomicExpression = (
        "rhai::runtime::atomic_write(``$atomicResultLiteral``, " +
        "``{`"state`":`"新`"}``)"
    )
    Invoke-Script @(
        'script', 'eval', $atomicExpression,
        '--profile', 'local'
    ) | Out-Null
    if ([IO.File]::ReadAllText($atomicResult) -ne '{"state":"新"}') {
        throw 'runtime atomic replacement did not publish the complete Unicode result'
    }
    $atomicStaging = @(
        Get-ChildItem -LiteralPath $runtimeDirectory -Force |
            Where-Object { $_.Name -like '.atomic-result-*.agenterm-atomic-*' }
    )
    if ($atomicStaging.Count -ne 0) {
        throw 'runtime atomic replacement left a sibling staging file'
    }
    $localPath = Invoke-Script @(
        'script', 'eval',
        "std::path::PathBuf::from(``$localDataLiteral``).extension",
        '--profile', 'local'
    )
    if ($localPath -ne 'json') {
        throw "local typed path returned unexpected extension: $localPath"
    }
    $localBytes = Invoke-Script @(
        'script', 'eval', 'rhai::bytes::from_text("hello").len',
        '--profile', 'local'
    )
    if ($localBytes -ne '5') {
        throw "local typed bytes returned unexpected length: $localBytes"
    }
    [IO.File]::WriteAllText(
        $sourceFile,
        "std::fs::read_to_string(``$localDataLiteral``)"
    )
    if ((Invoke-Script @(
        'script', 'check', $sourceFile, '--profile', 'local'
    )) -ne 'OK') {
        throw 'local check rejected a shipped qualified std API'
    }
    $legacyPureStd = Invoke-Script @(
        'script', 'eval', 'std::fs::exists(".")', '--profile', 'pure'
    )
    if ($legacyPureStd -ne 'true') {
        throw 'legacy pure spelling removed filesystem APIs from the unrestricted runtime'
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
    [IO.File]::WriteAllText($sourceFile, 'agent.workspace()')
    $migratedApi = Invoke-ScriptFailure 1 @(
        'script', 'check', $sourceFile, '--profile', 'observe'
    )
    if (-not $migratedApi.Contains('"code":"script_api_migrated"') -or
        -not $migratedApi.Contains('fleet.workspace.info()')) {
        throw 'Script API v2 did not provide a targeted agent-to-fleet migration diagnostic'
    }
    Write-Evidence 'script.rhai-runtime'

    Write-Host 'STEP unrestricted legacy spellings and operation budget'
    $legacyProcess = Invoke-Script @(
        'script', 'eval', 'std::process::id()', '--profile', 'pure'
    )
    if ([uint32]$legacyProcess -le 0) {
        throw 'legacy pure spelling removed process APIs from the unrestricted runtime'
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
    Write-Evidence 'script.rhai-robustness-budget'

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
        $cancelFrames[0].payload.failure.code -ne 'limit_cancelled' -or
        $cancelFrames[0].payload.exit_class -ne 'cancelled') {
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
    $abandonedTempOwnerPids.Add($interruptedClient.Id)
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
    $abandonedTempOwnerPids.Add($holderClients[0].Id)
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
        $abandonedTempOwnerPids.Add($holderClients[$index].Id)
        $holderClients[$index].Kill()
        $holderClients[$index].WaitForExit()
        Wait-ProcessGone -Id $workerId
        $holderClients[$index].Dispose()
    }
    $abandonedTempOwnerPids.Add($replacementClient.Id)
    $replacementClient.Kill()
    $replacementClient.WaitForExit()
    Wait-ProcessGone -Id $replacementWorker.Id
    $replacementClient.Dispose()
    $supervisorRecovered = Invoke-Script @('script', 'eval', '6 * 7')
    if ($supervisorRecovered -ne '42') {
        throw 'script supervisor did not recover after timeout/crash/interruption/concurrency'
    }
    $invocationTempParent = Join-Path $env:TEMP 'AgenTerm\script-invocations'
    if (Test-Path -LiteralPath $invocationTempParent) {
        $orphanTempRoots = @(
            Get-ChildItem -LiteralPath $invocationTempParent -Directory |
                Where-Object {
                    $ownerText = $_.Name.Split('-', 2)[0]
                    $ownerPid = 0
                    [int]::TryParse($ownerText, [ref]$ownerPid) -and
                        $abandonedTempOwnerPids.Contains($ownerPid)
                }
        )
        if ($orphanTempRoots.Count -ne 0) {
            throw (
                'recovery invocation did not prune abandoned owned temp roots: ' +
                (($orphanTempRoots | ForEach-Object Name) -join ', ')
            )
        }
    }
    Write-Evidence 'script.runtime-lifecycle'
    Write-Evidence 'script.supervisor'

    Write-Host 'STEP typed brokered observation'
    $env:AGENTERM_IPC_ADDRESS = $address
    $env:AGENTERM_WORKSPACE_PATH = $workspaceFile
    Invoke-Script @(
        '--address', $address, 'new-window', '-d', '-n', "script-observe-$PID"
    ) | Out-Null
    $guiStderr = Join-Path $smokeRun.RunDirectory 'fleet-gui-stderr.txt'
    $guiProcess = Start-Process -FilePath $guiExe -ArgumentList @(
        '--no-activate', '--address', $address
    ) -RedirectStandardError $guiStderr -PassThru
    Register-SmokeOwnedProcess -Context $smokeRun -Id $guiProcess.Id `
        -Kind 'gui' -Address $address
    if (-not $guiProcess.WaitForInputIdle(10000)) {
        throw 'Fleet observation GUI did not become input-idle within 10 seconds'
    }
    $snapshot = Invoke-Script @(
        'wait-ui', '--window-state', 'restored', '--timeout-ms', '10000'
    ) | ConvertFrom-Json
    $publicCapture = Invoke-Script @(
        'capture-pane', '-p', '-t', '@1', '--max-bytes', '5', '--json'
    ) | ConvertFrom-Json
    if ($publicCapture.max_bytes -ne 5 -or $publicCapture.bytes -gt 5) {
        throw 'public bounded capture did not return typed bounded metadata'
    }
    $invalidCapture = Invoke-ScriptFailure 1 @(
        'capture-pane', '-p', '-t', '@1', '--max-bytes', '0'
    )
    if (-not $invalidCapture.Contains('must be from 1 to 1048576')) {
        throw 'public bounded capture accepted an invalid byte limit'
    }
    $observed = Invoke-Script @(
        'script', 'eval', 'fleet.ui.snapshot().event_position.sequence',
        '--profile', 'observe'
    )
    if ([uint64]$observed -lt [uint64]$snapshot.event_position.sequence) {
        throw 'observe script did not receive the typed snapshot event position'
    }
    $workspace = Invoke-Script @(
        'script', 'eval', 'fleet.workspace.info()', '--profile', 'observe', '--json'
    ) | ConvertFrom-Json
    if (-not $workspace.ok -or -not $workspace.value.event_position.epoch) {
        throw 'workspace observation did not include the stable event baseline'
    }
    $tabs = Invoke-Script @(
        'script', 'eval', 'fleet.tabs.list()', '--profile', 'observe', '--json'
    ) | ConvertFrom-Json
    if (-not $tabs.ok -or @($tabs.value).Count -lt 1) {
        throw 'typed tabs observation returned no terminal tabs'
    }
    $active = Invoke-Script @(
        'script', 'eval', 'fleet.tabs.active()', '--profile', 'observe', '--json'
    ) | ConvertFrom-Json
    if (-not $active.ok -or -not $active.value.active) {
        throw 'typed active-tab observation did not identify the active tab'
    }
    $capture = Invoke-Script @(
        'script', 'eval', 'fleet.terminal(`@1`).capture(32)', '--profile', 'observe', '--json'
    ) | ConvertFrom-Json
    if (-not $capture.ok -or $capture.value.bytes -gt 32 -or
        $capture.value.max_bytes -ne 32) {
        throw 'typed pane capture exceeded its requested byte boundary'
    }
    $eventEpochLiteral = ConvertTo-Json -Compress ([string]$snapshot.event_position.epoch)
    $eventReadExpression = "fleet.events.read($eventEpochLiteral, $($snapshot.event_position.sequence), 16)"
    $eventBatch = Invoke-Script @(
        'script', 'eval', $eventReadExpression,
        '--profile', 'observe', '--json'
    ) | ConvertFrom-Json
    if (-not $eventBatch.ok -or -not $eventBatch.value.position) {
        throw 'typed event read did not return an observable-fleet position'
    }
    $restart = Invoke-ScriptFailure 6 @(
        'script', 'eval', 'fleet.events.read(`wrong-epoch`, 0, 1)',
        '--profile', 'observe'
    )
    if (-not $restart.Contains('"code":"server_restart"') -or
        -not $restart.Contains('"exit_class":"fleet"')) {
        throw 'typed event read did not preserve the stable restart error'
    }
    Write-Evidence 'script.exit-classes'
    $waitStopwatch = [Diagnostics.Stopwatch]::StartNew()
    $eventWaitExpression = "fleet.events.wait($eventEpochLiteral, $($snapshot.event_position.sequence), ""never.matches"", 50)"
    $waitTimeout = Invoke-ScriptFailure 6 @(
        'script', 'eval', $eventWaitExpression,
        '--profile', 'observe', '--timeout-ms', '200'
    )
    $waitStopwatch.Stop()
    if (-not $waitTimeout.Contains('event_wait_timeout') -or
        $waitStopwatch.ElapsedMilliseconds -ge 500) {
        throw 'broker wait did not remain inside the host deadline'
    }
    $legacyMutation = Invoke-Script @(
        'script', 'eval', 'fleet.ui.tabs.toggle().post_state.verified',
        '--profile', 'observe'
    )
    $legacyRestore = Invoke-Script @(
        'script', 'eval', 'fleet.ui.tabs.toggle().post_state.verified',
        '--profile', 'observe'
    )
    if ($legacyMutation -ne 'true' -or $legacyRestore -ne 'true') {
        throw 'legacy observe spelling removed Fleet mutation from the unrestricted runtime'
    }
    Write-Evidence 'script.rhai-fleet'

    Write-Host 'STEP Script API v2 Fleet mutation receipt, event, and post-state'
    $fleetBaselineVisible = [bool]$snapshot.layout.sidebar.visible
    $fleetMutationExpression = @'
let receipt = fleet.ui.tabs.toggle();
#{
    request_id: receipt.request_id,
    operation_id: receipt.operation_id,
    outcome: receipt.outcome,
    event_count: receipt.events.len(),
    event_kind: if receipt.events.is_empty() { "" } else { receipt.events[0].kind },
    verified: receipt.post_state.verified,
    reason: receipt.post_state.reason
}
'@
    $fleetMutation = Invoke-Script @(
        'script', 'eval', $fleetMutationExpression,
        '--profile', 'local', '--json'
    ) | ConvertFrom-Json
    $fleetMutationSnapshot = Invoke-Script @('ui-snapshot') | ConvertFrom-Json
    if (-not $fleetMutation.ok -or
        [string]::IsNullOrWhiteSpace($fleetMutation.value.request_id) -or
        $fleetMutation.value.operation_id -ne 'ui.tabs.toggle' -or
        $fleetMutation.value.outcome -notin @('committed', 'no_op') -or
        $fleetMutation.value.event_count -lt 1 -or
        $fleetMutation.value.event_kind -ne 'layout.tabs.visibility' -or
        -not $fleetMutation.value.verified -or
        [bool]$fleetMutationSnapshot.layout.sidebar.visible -eq
            $fleetBaselineVisible) {
        throw (
            'Fleet mutation did not expose its receipt, correlated event, and ' +
            'verified post-state: ' +
            ($fleetMutation.value | ConvertTo-Json -Compress -Depth 10)
        )
    }
    $fleetRestore = Invoke-Script @(
        'script', 'eval', 'fleet.ui.tabs.toggle().post_state.verified',
        '--profile', 'local'
    )
    $fleetRestoredSnapshot = Invoke-Script @('ui-snapshot') | ConvertFrom-Json
    if ($fleetRestore -ne 'true' -or
        [bool]$fleetRestoredSnapshot.layout.sidebar.visible -ne $fleetBaselineVisible) {
        throw 'Fleet mutation fixture did not restore its isolated UI state'
    }
    $activeFleetTab = @($fleetRestoredSnapshot.tabs | Where-Object active)[0]
    $fleetTabIdLiteral = ConvertTo-Json -Compress ([string]$activeFleetTab.id)
    $fleetNote = "Script API v2 目录 $PID"
    $fleetNoteLiteral = ConvertTo-Json -Compress $fleetNote
    $fleetOriginalNoteLiteral = ConvertTo-Json -Compress ([string]$activeFleetTab.note)
    $fleetNoteExpression = @"
let receipt = fleet.tabs.set_note($fleetTabIdLiteral, $fleetNoteLiteral);
#{
    operation_id: receipt.operation_id,
    outcome: receipt.outcome,
    event_count: receipt.events.len(),
    event_kind: if receipt.events.is_empty() { "" } else { receipt.events[0].kind },
    verified: receipt.post_state.verified,
    reason: receipt.post_state.reason,
    tab: receipt.post_state.value.id,
    note: receipt.post_state.value.note
}
"@
    $fleetNoteMutation = Invoke-Script @(
        'script', 'eval', $fleetNoteExpression,
        '--profile', 'local', '--json'
    ) | ConvertFrom-Json
    if (-not $fleetNoteMutation.ok -or
        $fleetNoteMutation.value.operation_id -ne 'tabs.set-note' -or
        $fleetNoteMutation.value.outcome -notin @('committed', 'no_op') -or
        $fleetNoteMutation.value.event_count -lt 1 -or
        $fleetNoteMutation.value.event_kind -ne 'tab.note' -or
        -not $fleetNoteMutation.value.verified -or
        $fleetNoteMutation.value.tab -ne $activeFleetTab.id -or
        $fleetNoteMutation.value.note -ne $fleetNote) {
        throw 'typed tab-note mutation lacked receipt, causal event, or verified post-state'
    }
    $fleetNoteRestoreExpression = (
        "fleet.tabs.set_note($fleetTabIdLiteral, " +
        "$fleetOriginalNoteLiteral).post_state.verified"
    )
    if ((Invoke-Script @(
        'script', 'eval', $fleetNoteRestoreExpression, '--profile', 'local'
    )) -ne 'true') {
        throw 'typed tab-note mutation did not restore the isolated fixture'
    }
    Write-Evidence 'script.fleet-tabs-set-note'
    Write-Evidence 'script.fleet-v2'

    Write-Host 'STEP direct agenterm-script north-star named task'
    $northStarSource = Join-Path $repositoryRoot 'examples\script-daily-check'
    $northStarProject = Join-Path $runtimeDirectory 'north-star-project'
    New-Item -ItemType Directory -Path $northStarProject -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $northStarSource 'agenterm.tasks.json') `
        -Destination $northStarProject
    Copy-Item -LiteralPath (Join-Path $northStarSource 'daily-check.rhai') `
        -Destination $northStarProject
    $northStarManifest = Join-Path $northStarProject 'agenterm.tasks.json'
    $northStarEntry = Join-Path $northStarProject 'daily-check.rhai'
    $northStarResultPath = Join-Path $runtimeDirectory 'daily-check-result.json'
    $northStarReadyPath = Join-Path $runtimeDirectory 'north-star-http-ready.json'
    $northStarLogPath = Join-Path $runtimeDirectory 'north-star-http-log.jsonl'
    $northStarStopPath = Join-Path $runtimeDirectory 'north-star-http-stop'
    $northStarHttp = Start-HttpFixture -FixturePath $httpFixtureScript `
        -ReadyPath $northStarReadyPath -LogPath $northStarLogPath `
        -StopPath $northStarStopPath
    try {
        $northStarReady = Wait-HttpFixtureReady -Process $northStarHttp `
            -ReadyPath $northStarReadyPath
        $northStarSnapshot = Invoke-Script @('ui-snapshot') | ConvertFrom-Json
        $northStarTab = @($northStarSnapshot.tabs | Where-Object active)[0]
        $northStarOriginalNote = [string]$northStarTab.note
        $northStarConfig = [ordered]@{
            schema_version = 1
            message = 'AgenTerm daily check：目录与终端'
            child_program = $Exe
            child_a_args = @('--version')
            child_b_args = @('list-commands')
            loopback_url = "$($northStarReady.url)/status"
            tab_id = [string]$northStarTab.id
            result_path = $northStarResultPath
        }
        [IO.File]::WriteAllText(
            (Join-Path $northStarProject 'config.json'),
            ($northStarConfig | ConvertTo-Json -Depth 8),
            [Text.UTF8Encoding]::new($false)
        )

        $directHelp = Invoke-DirectScript @('--help')
        if ($directHelp -notlike '*AgenTerm Script Runtime*' -or
            $directHelp -notlike '*agenterm-script task run*') {
            throw 'agenterm-script.exe did not expose its public CLI help'
        }
        $directCheck = Invoke-DirectScript @(
            'check', $northStarEntry, '--profile', 'local',
            '--project-root', $northStarProject
        )
        if ($directCheck -ne 'OK') {
            throw 'direct agenterm-script check rejected the north-star source'
        }
        $northStarCatalog = Invoke-DirectScript @(
            'task', 'list', '--manifest', $northStarManifest, '--json'
        ) | ConvertFrom-Json
        $northStarShow = Invoke-DirectScript @(
            'task', 'show', 'daily-check',
            '--manifest', $northStarManifest, '--json'
        ) | ConvertFrom-Json
        if ($northStarCatalog.project_id -ne 'agenterm-script-daily-check' -or
            $northStarCatalog.schema_version -ne 2 -or
            $northStarCatalog.script_api_version -ne 2 -or
            $northStarCatalog.script_catalog_schema_version -ne 3 -or
            $northStarCatalog.origin.kind -ne 'repository' -or
            $northStarCatalog.origin.id -ne 'agenterm' -or
            $northStarCatalog.provenance.producer -ne 'agenterm-example' -or
            $northStarCatalog.provenance.revision -ne 'daily-check-1' -or
            -not $northStarCatalog.compatible -or
            @($northStarCatalog.requirements.capabilities) -notcontains
                'rhai.http.start' -or
            @($northStarCatalog.tasks).Count -ne 1 -or
            $northStarCatalog.tasks[0].status -ne 'ready' -or
            -not $northStarShow.compatible -or
            $northStarShow.origin.id -ne 'agenterm' -or
            $northStarShow.provenance.revision -ne 'daily-check-1' -or
            $northStarShow.tasks[0].entry -ne 'daily-check.rhai' -or
            $northStarShow.tasks[0].profile -ne 'local') {
            throw 'north-star task list/show lost inspectable manifest facts'
        }
        $northStarTaskCheck = Invoke-DirectScript @(
            'task', 'check', 'daily-check',
            '--manifest', $northStarManifest
        )
        if ($northStarTaskCheck -ne 'OK') {
            throw 'agenterm-script task check rejected the compatible north-star task'
        }
        Write-Evidence 'script.direct-entry'

        $northStarRun = Invoke-DirectScript @(
            'task', 'run', 'daily-check',
            '--manifest', $northStarManifest,
            '--timeout-ms', '10000', '--max-operations', '1000000',
            '--json', '--', 'smoke-target'
        ) | ConvertFrom-Json
        $northStar = $northStarRun.value
        if (-not $northStarRun.ok -or
            $northStar.schema_version -ne 1 -or
            $northStar.target -ne 'smoke-target' -or
            $northStar.message -ne 'AgenTerm daily check：目录与终端' -or
            $northStar.child_a.pid -le 0 -or
            $northStar.child_b.pid -le 0 -or
            $northStar.child_a.pid -eq $northStar.child_b.pid -or
            $northStar.child_a.exit_code -ne 0 -or
            $northStar.child_b.exit_code -ne 0 -or
            -not $northStar.child_a.complete -or
            -not $northStar.child_b.complete -or
            $northStar.child_a.stdout -notlike '*agenterm-cli*' -or
            $northStar.child_b.stdout -notlike '*list-commands*' -or
            $northStar.http.status -ne 201 -or
            $northStar.http.body -ne 'hello' -or
            $northStar.http.task_state -ne 'completed' -or
            $northStar.fleet.operation_id -ne 'tabs.set-note' -or
            $northStar.fleet.event_kind -ne 'tab.note' -or
            -not $northStar.fleet.verified) {
            throw 'north-star task did not close its process/HTTP/Fleet result loop'
        }
        if (Test-Path -LiteralPath $northStar.temp_root) {
            throw 'north-star invocation-owned temporary root survived task completion'
        }
        if (-not (Test-Path -LiteralPath $northStarResultPath)) {
            throw 'north-star task did not atomically publish its result'
        }
        $northStarPersisted = [IO.File]::ReadAllText($northStarResultPath) |
            ConvertFrom-Json
        if ($northStarPersisted.target -ne $northStar.target -or
            $northStarPersisted.fleet.note -ne $northStar.fleet.note) {
            throw 'north-star atomic result disagreed with the returned aggregate'
        }
        $northStarStaging = @(
            Get-ChildItem -LiteralPath $runtimeDirectory -Force |
                Where-Object { $_.Name -like '.daily-check-result*.agenterm-atomic-*' }
        )
        if ($northStarStaging.Count -ne 0) {
            throw 'north-star atomic result left a staging file'
        }
        foreach ($childPid in @(
            [int]$northStar.child_a.pid,
            [int]$northStar.child_b.pid
        )) {
            if (Get-Process -Id $childPid -ErrorAction SilentlyContinue) {
                throw "north-star child process survived task completion: $childPid"
            }
        }
        $restoreTabLiteral = ConvertTo-Json -Compress ([string]$northStarTab.id)
        $restoreNoteLiteral = ConvertTo-Json -Compress $northStarOriginalNote
        $restoreNorthStar = (
            "fleet.tabs.set_note($restoreTabLiteral, " +
            "$restoreNoteLiteral).post_state.verified"
        )
        if ((Invoke-Script @(
            'script', 'eval', $restoreNorthStar, '--profile', 'local'
        )) -ne 'true') {
            throw 'north-star task did not restore its isolated Fleet fixture'
        }
        Write-Evidence 'script.north-star'
    }
    finally {
        [IO.File]::WriteAllText($northStarStopPath, 'stop')
        if (-not $northStarHttp.WaitForExit(3000)) {
            $northStarHttp.Kill()
            $northStarHttp.WaitForExit()
        }
        $northStarHttp.Dispose()
    }

    Write-Host 'STEP privacy-bounded reusable script audit'
    [IO.File]::WriteAllText(
        $sourceFile,
        'print("AUDIT_STDOUT_SECRET"); args[0] + "AUDIT_SOURCE_SECRET"'
    )
    $auditSecretResult = Invoke-Script @(
        'script', 'run', $sourceFile, '--', 'AUDIT_ARG_SECRET'
    )
    if (-not $auditSecretResult.Contains('AUDIT_STDOUT_SECRET') -or
        -not $auditSecretResult.Contains('AUDIT_ARG_SECRETAUDIT_SOURCE_SECRET')) {
        throw 'audit privacy fixture did not exercise source/stdout/argv secrets'
    }
    $auditText = [IO.File]::ReadAllText($auditFile)
    foreach ($secret in @(
        'AUDIT_STDOUT_SECRET',
        'AUDIT_ARG_SECRET',
        'AUDIT_SOURCE_SECRET',
        'AUDIT_ENV_SECRET',
        'HTTP_CREDENTIAL_SECRET',
        'PRIVATE_PATH_SECRET',
        'PROXY_CREDENTIAL_SECRET'
    )) {
        if ($auditText.Contains($secret)) {
            throw "script audit leaked forbidden value: $secret"
        }
    }
    foreach ($forbiddenField in @(
        '"source":',
        '"arguments":',
        '"argv":',
        '"stdout":',
        '"pane":',
        '"environment":',
        '"clipboard":',
        '"credentials":'
    )) {
        if ($auditText.Contains($forbiddenField)) {
            throw "script audit exposed forbidden field: $forbiddenField"
        }
    }
    $auditRecords = @(
        [IO.File]::ReadAllLines($auditFile) |
            Where-Object { -not [string]::IsNullOrWhiteSpace($_) } |
            ForEach-Object { $_ | ConvertFrom-Json }
    )
    $auditFailureCodes = @($auditRecords | ForEach-Object { $_.failure_code })
    if ($auditRecords.Count -lt 10 -or
        $auditFailureCodes -notcontains 'script_api_unavailable' -or
        $auditFailureCodes -notcontains 'limit_wall_time' -or
        $auditFailureCodes -notcontains 'host_hard_timeout' -or
        $auditFailureCodes -notcontains 'host_worker_crash' -or
        $auditFailureCodes -notcontains 'host_concurrency_limit' -or
        @($auditRecords | Where-Object { $_.denied }).Count -lt 1 -or
        @($auditRecords | Where-Object { $_.cancelled -and $_.timed_out }).Count -lt 1 -or
        @($auditRecords | Where-Object { $_.crashed }).Count -lt 1 -or
        @($auditRecords | Where-Object {
            $_.requested_profile -eq 'observe' -and
            $_.effective_profile -eq 'unrestricted' -and
            $_.effective_capabilities -contains 'unrestricted_local' -and
            $_.broker_operation_ids -contains 'ui.snapshot'
        }).Count -lt 1 -or
        @($auditRecords | Where-Object {
            $_.requested_profile -eq 'pure' -and
            $_.effective_profile -eq 'unrestricted' -and
            $_.effective_capabilities -contains 'unrestricted_local'
        }).Count -lt 1 -or
        @($auditRecords | Where-Object {
            $_.source_fingerprint -match '^fnv1a128:[0-9a-f]{32}$'
        }).Count -ne $auditRecords.Count) {
        throw 'script audit did not capture the required bounded result metadata'
    }
    $env:AGENTERM_SCRIPT_AUDIT_PATH = $env:TEMP
    $auditWriteFailure = Invoke-ScriptFailure 1 @('script', 'eval', '1')
    $env:AGENTERM_SCRIPT_AUDIT_PATH = $auditFile
    if (-not $auditWriteFailure.Contains('"code":"host_audit_write"')) {
        throw 'script audit write failure did not fail closed with a typed host error'
    }
    Write-Evidence 'script.audit'

    $runSucceeded = $true
    Write-Host 'PASS: unrestricted scripting API, supervision, audit privacy, and budgets'
}
catch {
    $runFailure = if ($InternalFailureBundleProbe) {
        @(
            "INTERNAL_FAILURE_BUNDLE_PROBE:script:$($smokeRun.RunId)"
            'intentional script failure-bundle probe'
        ) -join "`n"
    }
    else {
        Protect-ScriptSmokeDiagnosticText -Text ($_ | Out-String)
    }
    throw
}
finally {
    foreach ($client in $script:ownedScriptClients) {
        try {
            if (-not $client.HasExited) {
                $client.Kill()
                $client.WaitForExit()
            }
        }
        catch {
            # A scenario may already have disposed its owned client.
        }
    }
    Remove-Item -LiteralPath $sourceFile -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $auditFile -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath "$auditFile.1" -ErrorAction SilentlyContinue
    Protect-ScriptSmokeCommandLog
    $env:AGENTERM_IPC_ADDRESS = $address
    $env:AGENTERM_WORKSPACE_PATH = $workspaceFile
    try {
        Complete-SmokeRun -Context $smokeRun -Succeeded $runSucceeded `
            -FailureRecord $runFailure
    }
    finally {
        if ($null -eq $previousAuditPath) {
            Remove-Item Env:AGENTERM_SCRIPT_AUDIT_PATH -ErrorAction SilentlyContinue
        }
        else {
            $env:AGENTERM_SCRIPT_AUDIT_PATH = $previousAuditPath
        }
        if ($null -eq $previousAuditSecret) {
            Remove-Item Env:AGENTERM_AUDIT_ENV_SECRET -ErrorAction SilentlyContinue
        }
        else {
            $env:AGENTERM_AUDIT_ENV_SECRET = $previousAuditSecret
        }
    }
}
