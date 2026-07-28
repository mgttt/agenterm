param(
    [string]$Exe = (Join-Path $PSScriptRoot '..\dist\agenterm-cli.exe'),
    [switch]$ListEvidence,
    [switch]$InternalFailureBundleProbe
)

$ErrorActionPreference = 'Stop'
$declaredEvidence = @(
    'script.rhai-pure'
    'script.rhai-observe'
    'script.rhai-deny-budget'
    'script.rhai-framed'
    'script.modules-tasks'
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

function Invoke-Script {
    param([string[]]$CommandArgs)
    Invoke-SmokeCli -Context $smokeRun -Arguments $CommandArgs
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
        'AUDIT_ENV_SECRET'
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
        $apiResult.value.api_version -ne 1 -or
        $apiResult.value.schema_version -ne 2 -or
        $apiResult.value.default_profile -ne 'local' -or
        $apiResult.value.profiles.pure.variables -notcontains 'args' -or
        $apiResult.value.profiles.observe.variables -notcontains 'agent' -or
        $apiResult.value.profiles.local.status -ne 'shipped' -or
        $apiResult.value.limits.defaults.wall_time_ms -ne 2000 -or
        $apiResult.value.limits.hard_maximums.wall_time_ms -ne 10000 -or
        $apiResult.value.limits.invocation_bytes -ne 2097152 -or
        $apiResult.value.framing.version -ne 1 -or
        $apiResult.value.framing.max_frame_bytes -ne 2097152 -or
        $apiResult.value.framing.input_kinds.broker_request -ne 'available_worker_to_host' -or
        $apiResult.value.limits.defaults.broker_requests -ne 64 -or
        $apiResult.value.limits.hard_maximums.capture_bytes -ne 262144 -or
        $apiResult.value.supervisor.job_object -ne 'kill_on_close' -or
        $apiResult.value.supervisor.global_concurrency -ne 4 -or
        $apiResult.value.exit_classes.limit -ne 3 -or
        $apiResult.value.failure_categories -notcontains 'protocol' -or
        @($apiResult.value.entries | Where-Object {
            $_.stable_id -eq 'fleet.tabs.new' -and $_.status -eq 'planned'
        }).Count -ne 1 -or
        @($apiResult.value.entries | Where-Object {
            $_.surface_path -eq 'agent.workspace' -and
            $_.status -eq 'shipped' -and
            $_.catalog_path -eq 'fleet/workspace/get' -and
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
    if ($taskCatalog.schema_version -ne 1 -or
        $taskCatalog.project_id -ne 'script-smoke' -or
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
        @($taskShow.tasks).Count -ne 1 -or
        $taskShow.tasks[0].entry -ne 'main.rhai') {
        throw 'named-task inspection lost project or entry identity'
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
    $pureStdDenied = Invoke-ScriptFailure 1 @(
        'script', 'eval', 'std::fs::exists(".")', '--profile', 'pure'
    )
    if (-not $pureStdDenied.Contains('"code":"script_runtime"')) {
        throw 'pure profile unexpectedly received local filesystem authority'
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
    $denied = Invoke-ScriptFailure 1 @(
        'script', 'eval', 'agent.workspace()', '--profile', 'pure'
    )
    if (-not $denied.Contains('"code":"script_runtime"')) {
        throw 'pure profile unexpectedly received broker authority'
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

    Write-Host 'STEP typed brokered observation'
    $env:AGENTERM_IPC_ADDRESS = $address
    $env:AGENTERM_WORKSPACE_PATH = $workspaceFile
    Invoke-Script @(
        '--address', $address, 'new-window', '-d', '-n', "script-observe-$PID"
    ) | Out-Null
    $snapshot = Invoke-Script @('ui-snapshot') | ConvertFrom-Json
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
        'script', 'eval', 'agent.ui_snapshot().event_position.sequence',
        '--profile', 'observe'
    )
    if ([uint64]$observed -lt [uint64]$snapshot.event_position.sequence) {
        throw 'observe script did not receive the typed snapshot event position'
    }
    $workspace = Invoke-Script @(
        'script', 'eval', 'agent.workspace()', '--profile', 'observe', '--json'
    ) | ConvertFrom-Json
    if (-not $workspace.ok -or -not $workspace.value.event_position.epoch) {
        throw 'workspace observation did not include the stable event baseline'
    }
    $tabs = Invoke-Script @(
        'script', 'eval', 'agent.tabs()', '--profile', 'observe', '--json'
    ) | ConvertFrom-Json
    if (-not $tabs.ok -or @($tabs.value).Count -lt 1) {
        throw 'typed tabs observation returned no terminal tabs'
    }
    $active = Invoke-Script @(
        'script', 'eval', 'agent.active_tab()', '--profile', 'observe', '--json'
    ) | ConvertFrom-Json
    if (-not $active.ok -or -not $active.value.active) {
        throw 'typed active-tab observation did not identify the active tab'
    }
    $capture = Invoke-Script @(
        'script', 'eval', 'agent.capture(`@1`, 32)', '--profile', 'observe', '--json'
    ) | ConvertFrom-Json
    if (-not $capture.ok -or $capture.value.bytes -gt 32 -or
        $capture.value.max_bytes -ne 32) {
        throw 'typed pane capture exceeded its requested byte boundary'
    }
    $eventEpochLiteral = ConvertTo-Json -Compress ([string]$snapshot.event_position.epoch)
    $eventReadExpression = "agent.events_read($eventEpochLiteral, $($snapshot.event_position.sequence), 16)"
    $eventBatch = Invoke-Script @(
        'script', 'eval', $eventReadExpression,
        '--profile', 'observe', '--json'
    ) | ConvertFrom-Json
    if (-not $eventBatch.ok -or -not $eventBatch.value.position) {
        throw 'typed event read did not return an observable-fleet position'
    }
    $restart = Invoke-ScriptFailure 1 @(
        'script', 'eval', 'agent.events_read(`wrong-epoch`, 0, 1)',
        '--profile', 'observe'
    )
    if (-not $restart.Contains('server_restart')) {
        throw 'typed event read did not preserve the stable restart error'
    }
    $waitStopwatch = [Diagnostics.Stopwatch]::StartNew()
    $eventWaitExpression = "agent.events_wait($eventEpochLiteral, $($snapshot.event_position.sequence), ""never.matches"", 50)"
    $waitTimeout = Invoke-ScriptFailure 1 @(
        'script', 'eval', $eventWaitExpression,
        '--profile', 'observe', '--timeout-ms', '200'
    )
    $waitStopwatch.Stop()
    if (-not $waitTimeout.Contains('event_wait_timeout') -or
        $waitStopwatch.ElapsedMilliseconds -ge 500) {
        throw 'broker wait did not remain inside the host deadline'
    }
    $mutationDenied = Invoke-ScriptFailure 1 @(
        'script', 'eval', 'new_tab()', '--profile', 'observe'
    )
    if (-not $mutationDenied.Contains('"code":"script_runtime"')) {
        throw 'observe profile unexpectedly exposed a mutation API'
    }
    Write-Evidence 'script.rhai-observe'

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
        'AUDIT_ENV_SECRET'
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
            $_.effective_profile -eq 'observe' -and
            $_.effective_capabilities -contains 'observe' -and
            $_.broker_operation_ids -contains 'ui.snapshot'
        }).Count -lt 1 -or
        @($auditRecords | Where-Object {
            $_.effective_profile -eq 'local' -and
            $_.effective_capabilities -contains 'local'
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
    Write-Host 'PASS: safe scripting API, supervision, audit privacy, denial, and budgets'
}
catch {
    $runFailure = Protect-ScriptSmokeDiagnosticText -Text ($_ | Out-String)
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
