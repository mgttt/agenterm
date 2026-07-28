$script:SmokeCommandOutputLimit = 64KB
$script:SmokeCommandLogLimit = 512KB
$script:SmokeDiagnosticOutputLimit = 64KB

function Limit-SmokeText {
    param(
        [AllowNull()][string]$Text,
        [Parameter(Mandatory = $true)][int]$MaximumBytes
    )

    if ($null -eq $Text) {
        return ''
    }
    $encoding = [Text.UTF8Encoding]::new($false)
    $bytes = $encoding.GetBytes($Text)
    if ($bytes.Length -le $MaximumBytes) {
        return $Text
    }
    $length = $MaximumBytes
    while ($length -gt 0) {
        try {
            $prefix = [Text.UTF8Encoding]::new($false, $true).GetString(
                $bytes, 0, $length
            )
            return "$prefix`n<diagnostic truncated at $MaximumBytes UTF-8 bytes>"
        }
        catch {
            $length--
        }
    }
    return "<diagnostic truncated at $MaximumBytes UTF-8 bytes>"
}

function Get-SmokeLoopbackAddress {
    $listener = [Net.Sockets.TcpListener]::new(
        [Net.IPAddress]::Loopback, 0
    )
    try {
        $listener.Start()
        $port = ([Net.IPEndPoint]$listener.LocalEndpoint).Port
    }
    finally {
        $listener.Stop()
    }
    return "127.0.0.1:$port"
}

function ConvertTo-SmokeSafeArguments {
    param([Parameter(Mandatory = $true)][string[]]$Arguments)

    $sensitiveValueOptions = @(
        '-e', '--env', '--proxy', '--no-proxy', '--proxy-input', '--path'
    )
    $contentCommands = @(
        'script', 'send-keys', 'send', 'set-composer', 'set-tab-note'
    )
    $safe = [Collections.Generic.List[string]]::new()
    $redactPositionals = (
        $Arguments.Count -gt 0 -and $contentCommands -contains $Arguments[0]
    )
    $position = 0
    while ($position -lt $Arguments.Count) {
        $argument = $Arguments[$position]
        $safe.Add($argument)
        if ($sensitiveValueOptions -contains $argument) {
            if ($position + 1 -lt $Arguments.Count) {
                $safe.Add('<redacted>')
                $position += 2
                continue
            }
        }
        if ($redactPositionals -and $position -gt 0 -and
            -not $argument.StartsWith('-') -and
            $safe[$safe.Count - 1] -ne '<redacted>') {
            $safe[$safe.Count - 1] = '<content>'
        }
        $position++
    }
    return @($safe)
}

function Add-SmokeCommandRecord {
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)][string[]]$Arguments,
        [Parameter(Mandatory = $true)][int]$ExitCode,
        [Parameter(Mandatory = $true)][bool]$ExpectedFailure,
        [AllowNull()][string]$Output
    )

    if ($Context.CommandLogBytes -ge $script:SmokeCommandLogLimit) {
        return
    }
    $safeArguments = ConvertTo-SmokeSafeArguments -Arguments $Arguments
    $boundedOutput = Limit-SmokeText -Text $Output `
        -MaximumBytes $script:SmokeCommandOutputLimit
    $record = @(
        "[$([DateTime]::UtcNow.ToString('o'))]"
        "command=$($Context.Executable) $($safeArguments -join ' ')"
        "expected_failure=$ExpectedFailure exit_code=$ExitCode"
        'output:'
        $boundedOutput
        ''
    ) -join "`n"
    $encoding = [Text.UTF8Encoding]::new($false)
    $remaining = $script:SmokeCommandLogLimit - $Context.CommandLogBytes
    $record = Limit-SmokeText -Text $record -MaximumBytes $remaining
    Add-Content -LiteralPath $Context.CommandLogPath -Value $record -Encoding UTF8
    $Context.CommandLogBytes += $encoding.GetByteCount($record)
}

function Invoke-SmokeCli {
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)][string[]]$Arguments,
        [switch]$ExpectFailure
    )

    $savedPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $outputItems = @(& $Context.Executable @Arguments 2>&1)
        $exitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $savedPreference
    }
    $output = $outputItems -join "`n"
    Add-SmokeCommandRecord -Context $Context -Arguments $Arguments `
        -ExitCode $exitCode -ExpectedFailure ([bool]$ExpectFailure) -Output $output
    Sync-SmokeOwnedServers -Context $Context
    Sync-SmokeOwnedUiClients -Context $Context
    $safeCommand = (ConvertTo-SmokeSafeArguments -Arguments $Arguments) -join ' '

    if ($ExpectFailure) {
        if ($exitCode -eq 0) {
            throw "agenterm $safeCommand unexpectedly succeeded"
        }
    }
    elseif ($exitCode -ne 0) {
        throw "agenterm $safeCommand failed:`n$output"
    }
    return $output
}

function Write-SmokeEvidence {
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)][string]$Id
    )

    if ($Context.DeclaredEvidence -notcontains $Id) {
        throw "$($Context.Suite) emitted undeclared evidence ID: $Id"
    }
    Add-Content -LiteralPath $Context.EvidencePath -Value $Id -Encoding UTF8
    Write-Host "EVIDENCE $Id"
}

function Register-SmokeOwnedAddress {
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)][string]$Address
    )
    $Context.OwnedAddresses.Add($Address) | Out-Null
}

function Register-SmokeOwnedProcess {
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)][int]$Id,
        [Parameter(Mandatory = $true)][string]$Kind,
        [string]$Address = ''
    )

    if ($Id -le 0) {
        throw "Refusing to register invalid owned PID: $Id"
    }
    $process = Get-Process -Id $Id -ErrorAction SilentlyContinue
    if ($null -eq $process) {
        return
    }
    $existing = @($Context.OwnedProcesses |
        Where-Object { $_.pid -eq $Id }) | Select-Object -First 1
    $windowHandle = [int64]$process.MainWindowHandle
    if ($null -ne $existing) {
        if ($Kind -eq 'server') {
            $existing.kind = 'server'
        }
        if ($windowHandle -ne 0) {
            $existing.window_handle = $windowHandle
        }
        if (-not [string]::IsNullOrWhiteSpace($Address)) {
            $existing.address = $Address
            Register-SmokeOwnedAddress -Context $Context -Address $Address
        }
        return
    }
    $startTime = try {
        $process.StartTime.ToUniversalTime().ToString('o')
    } catch {
        ''
    }
    $Context.OwnedProcesses.Add([pscustomobject]@{
        pid = $Id
        kind = $Kind
        address = $Address
        process_name = $process.ProcessName
        start_time_utc = $startTime
        window_handle = $windowHandle
        forced = $false
    })
    if (-not [string]::IsNullOrWhiteSpace($Address)) {
        Register-SmokeOwnedAddress -Context $Context -Address $Address
    }
}

function Sync-SmokeOwnedServers {
    param([Parameter(Mandatory = $true)]$Context)

    foreach ($recordPath in @(
        Get-ChildItem -LiteralPath $Context.InstanceDirectory -File `
            -Filter '*.json' -ErrorAction SilentlyContinue
    )) {
        try {
            $record = Get-Content -LiteralPath $recordPath.FullName -Raw |
                ConvertFrom-Json
            $address = [string]$record.address
            if ($Context.OwnedAddresses.Contains($address)) {
                Register-SmokeOwnedProcess -Context $Context `
                    -Id ([int]$record.pid) -Kind 'server' -Address $address
            }
        }
        catch {
            # A concurrently retiring registration is handled by final cleanup.
        }
    }
}

function Sync-SmokeOwnedUiClients {
    param([Parameter(Mandatory = $true)]$Context)

    foreach ($address in @($Context.OwnedAddresses)) {
        $savedPreference = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        try {
            $snapshotItems = @(
                & $Context.Executable --address $address ui-snapshot 2>$null
            )
            $exitCode = $LASTEXITCODE
        }
        catch {
            $snapshotItems = @()
            $exitCode = -1
        }
        finally {
            $ErrorActionPreference = $savedPreference
        }
        if ($exitCode -ne 0 -or $snapshotItems.Count -eq 0) {
            continue
        }
        try {
            $snapshot = ($snapshotItems -join "`n") | ConvertFrom-Json
            if (
                [string]$snapshot.projection -ne 'replaceable_ui_client' -or
                [int]$snapshot.client_pid -le 0
            ) {
                continue
            }
            $clientPid = [int]$snapshot.client_pid
            $client = Get-CimInstance Win32_Process `
                -Filter "ProcessId=$clientPid" -ErrorAction SilentlyContinue
            if ($null -eq $client) {
                continue
            }
            $expectedGui = [IO.Path]::GetFullPath(
                (Join-Path (Split-Path $Context.Executable -Parent) 'agenterm.exe')
            )
            if (
                -not [string]::Equals(
                    [string]$client.ExecutablePath,
                    $expectedGui,
                    [StringComparison]::OrdinalIgnoreCase
                ) -or
                [string]$client.CommandLine -notmatch
                    ('(?i)(?:--address\s+|--address=)' +
                        [regex]::Escape($address) + '(?:\s|$)')
            ) {
                throw (
                    "Refusing to own UI client PID $clientPid because its " +
                    "executable or address does not match this smoke run."
                )
            }
            Register-SmokeOwnedProcess -Context $Context -Id $clientPid `
                -Kind 'gui' -Address $address
        }
        catch {
            if ($_.Exception.Message -like 'Refusing to own UI client*') {
                throw
            }
            # A client detaching while its snapshot is read is harmless.
        }
    }
}

function Invoke-SmokeDiagnostic {
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string[]]$Arguments
    )

    $savedPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $items = @(& $Context.Executable @Arguments 2>&1)
        $exitCode = $LASTEXITCODE
    }
    catch {
        $items = @($_ | Out-String)
        $exitCode = -1
    }
    finally {
        $ErrorActionPreference = $savedPreference
    }
    $text = "exit_code=$exitCode`n" + ($items -join "`n")
    $text = Limit-SmokeText -Text $text `
        -MaximumBytes $script:SmokeDiagnosticOutputLimit
    $path = Join-Path $Context.FailureDirectory "$Name.txt"
    [IO.File]::WriteAllText($path, $text, [Text.UTF8Encoding]::new($false))
    return [pscustomobject]@{
        ExitCode = $exitCode
        Output = ($items -join "`n")
        Path = $path
    }
}

function Save-SmokeFailureBundle {
    param(
        [Parameter(Mandatory = $true)]$Context,
        [AllowNull()]$FailureRecord
    )

    New-Item -ItemType Directory -Path $Context.FailureDirectory `
        -Force | Out-Null
    $snapshot = Invoke-SmokeDiagnostic -Context $Context `
        -Name 'ui-snapshot' -Arguments @('ui-snapshot')
    if ($snapshot.ExitCode -eq 0 -and $Context.AllowPaneCapture) {
        Invoke-SmokeDiagnostic -Context $Context -Name 'capture-pane' `
            -Arguments @(
                'capture-pane', '-p', '--json', '--max-bytes', '16384'
            ) | Out-Null
        try {
            $state = $snapshot.Output | ConvertFrom-Json
            $epoch = [string]$state.event_position.epoch
            if (-not [string]::IsNullOrWhiteSpace($epoch)) {
                Invoke-SmokeDiagnostic -Context $Context -Name 'events' `
                    -Arguments @(
                        'read-events', '--epoch', $epoch, '--after', '0',
                        '--limit', '256'
                    ) | Out-Null
            }
        }
        catch {
            $parseFailure = Limit-SmokeText -Text ($_ | Out-String) `
                -MaximumBytes 8192
            [IO.File]::WriteAllText(
                (Join-Path $Context.FailureDirectory 'snapshot-parse.txt'),
                $parseFailure,
                [Text.UTF8Encoding]::new($false)
            )
        }
    }
    $failureText = if ($null -eq $FailureRecord) {
        'unknown failure'
    } else {
        Limit-SmokeText -Text ($FailureRecord | Out-String) -MaximumBytes 16384
    }
    $manifest = [ordered]@{
        schema_version = 1
        suite = $Context.Suite
        run_id = $Context.RunId
        started_at_utc = $Context.StartedAtUtc
        failed_at_utc = [DateTime]::UtcNow.ToString('o')
        address = $Context.Address
        executable = $Context.Executable
        failure = $failureText
        command_log = [IO.Path]::GetFileName($Context.CommandLogPath)
        evidence = [IO.Path]::GetFileName($Context.EvidencePath)
        diagnostics = @(
            Get-ChildItem -LiteralPath $Context.FailureDirectory -File |
                Select-Object -ExpandProperty Name
        )
        privacy = @{
            command_arguments = 'known content-bearing arguments redacted'
            output_limit_bytes = $script:SmokeDiagnosticOutputLimit
            pane_capture = if ($Context.AllowPaneCapture) {
                'explicitly enabled; bounded to 16384 bytes'
            } else {
                'disabled for this suite'
            }
        }
    }
    $manifest | ConvertTo-Json -Depth 6 |
        Set-Content -LiteralPath $Context.ManifestPath -Encoding UTF8
}

function Restore-SmokeEnvironment {
    param([Parameter(Mandatory = $true)]$Context)

    foreach ($entry in $Context.PreviousEnvironment.GetEnumerator()) {
        if ($null -eq $entry.Value) {
            Remove-Item "Env:$($entry.Key)" -ErrorAction SilentlyContinue
        } else {
            Set-Item "Env:$($entry.Key)" -Value $entry.Value
        }
    }
}

function Test-SmokeOwnedProcessIdentity {
    param(
        [Parameter(Mandatory = $true)]$Record,
        [Parameter(Mandatory = $true)][Diagnostics.Process]$Process
    )
    if ([string]::IsNullOrWhiteSpace([string]$Record.start_time_utc)) {
        return $false
    }
    try {
        return $Process.StartTime.ToUniversalTime().ToString('o') -eq
            [string]$Record.start_time_utc
    }
    catch {
        return $false
    }
}

function Stop-SmokeOwnedResources {
    param([Parameter(Mandatory = $true)]$Context)

    Sync-SmokeOwnedServers -Context $Context
    # Discover replaceable GUI clients before stopping their servers. Once the
    # server exits, its authoritative UI lease is no longer queryable.
    Sync-SmokeOwnedUiClients -Context $Context
    $graceful = [Collections.Generic.List[object]]::new()
    foreach ($address in @($Context.OwnedAddresses)) {
        $liveServer = @($Context.OwnedProcesses | Where-Object {
            $_.address -eq $address -and $_.kind -eq 'server'
        } | Where-Object {
            $candidate = Get-Process -Id $_.pid -ErrorAction SilentlyContinue
            $null -ne $candidate -and
                (Test-SmokeOwnedProcessIdentity -Record $_ -Process $candidate)
        })
        if ($liveServer.Count -eq 0) {
            continue
        }
        $savedPreference = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        try {
            $output = @(
                & $Context.Executable --address $address kill-server 2>&1
            )
            $exitCode = $LASTEXITCODE
        }
        catch {
            $output = @($_ | Out-String)
            $exitCode = -1
        }
        finally {
            $ErrorActionPreference = $savedPreference
        }
        $graceful.Add([ordered]@{
            address = $address
            exit_code = $exitCode
            output = Limit-SmokeText -Text ($output -join "`n") `
                -MaximumBytes 8192
        })
    }

    $deadline = [DateTime]::UtcNow.AddSeconds(3)
    do {
        $live = @($Context.OwnedProcesses | Where-Object {
            $candidate = Get-Process -Id $_.pid -ErrorAction SilentlyContinue
            $null -ne $candidate -and
                (Test-SmokeOwnedProcessIdentity -Record $_ -Process $candidate)
        })
        if ($live.Count -eq 0) {
            break
        }
        Start-Sleep -Milliseconds 50
    } while ([DateTime]::UtcNow -lt $deadline)

    $forcedPids = [Collections.Generic.List[int]]::new()
    $forceErrors = [Collections.Generic.List[string]]::new()
    foreach ($record in $Context.OwnedProcesses) {
        $process = Get-Process -Id $record.pid -ErrorAction SilentlyContinue
        if ($null -eq $process -or
            -not (Test-SmokeOwnedProcessIdentity -Record $record `
                -Process $process)) {
            continue
        }
        try {
            Stop-Process -Id $record.pid -Force -ErrorAction Stop
            $record.forced = $true
            $forcedPids.Add([int]$record.pid)
        }
        catch {
            $forceErrors.Add(
                "PID $($record.pid): $(($_ | Out-String).Trim())"
            )
        }
    }
    $forceDeadline = [DateTime]::UtcNow.AddSeconds(3)
    do {
        $remaining = @($Context.OwnedProcesses | Where-Object {
            $candidate = Get-Process -Id $_.pid -ErrorAction SilentlyContinue
            $null -ne $candidate -and
                (Test-SmokeOwnedProcessIdentity -Record $_ -Process $candidate)
        })
        if ($remaining.Count -eq 0) {
            break
        }
        Start-Sleep -Milliseconds 50
    } while ([DateTime]::UtcNow -lt $forceDeadline)

    $savedAddress = $env:AGENTERM_IPC_ADDRESS
    Remove-Item Env:AGENTERM_IPC_ADDRESS -ErrorAction SilentlyContinue
    try {
        & $Context.Executable server-list --json 2>$null | Out-Null
    }
    finally {
        if ($null -eq $savedAddress) {
            Remove-Item Env:AGENTERM_IPC_ADDRESS -ErrorAction SilentlyContinue
        } else {
            $env:AGENTERM_IPC_ADDRESS = $savedAddress
        }
    }

    $remainingRegistrations = @(
        Get-ChildItem -LiteralPath $Context.InstanceDirectory -File `
            -Filter '*.json' -ErrorAction SilentlyContinue
    )
    $remainingWindows = @($Context.OwnedProcesses | Where-Object {
        $candidate = Get-Process -Id $_.pid -ErrorAction SilentlyContinue
        [int64]$_.window_handle -ne 0 -and $null -ne $candidate -and
            (Test-SmokeOwnedProcessIdentity -Record $_ -Process $candidate)
    })
    $result = [ordered]@{
        schema_version = 1
        completed_at_utc = [DateTime]::UtcNow.ToString('o')
        owned_processes = @($Context.OwnedProcesses)
        graceful_shutdowns = @($graceful)
        forced_pids = @($forcedPids)
        force_errors = @($forceErrors)
        remaining_pids = @($remaining | ForEach-Object { [int]$_.pid })
        remaining_windows = @(
            $remainingWindows | ForEach-Object { [int64]$_.window_handle }
        )
        remaining_registrations = @(
            $remainingRegistrations | Select-Object -ExpandProperty Name
        )
        orphan_free = (
            $remaining.Count -eq 0 -and
            $remainingWindows.Count -eq 0 -and
            $remainingRegistrations.Count -eq 0 -and
            $forceErrors.Count -eq 0
        )
    }
    $result | ConvertTo-Json -Depth 7 |
        Set-Content -LiteralPath $Context.CleanupPath -Encoding UTF8
    Add-Content -LiteralPath $Context.EvidencePath `
        -Value "HARNESS cleanup.orphan-free=$($result.orphan_free)" `
        -Encoding UTF8
    Write-Host (
        "CLEANUP orphan_free=$($result.orphan_free) " +
        "owned=$($Context.OwnedProcesses.Count) forced=$($forcedPids.Count)"
    )
    if (-not $result.orphan_free) {
        throw 'Smoke cleanup left owned processes, windows, or registrations.'
    }
    return $result
}

function Complete-SmokeRun {
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)][bool]$Succeeded,
        [AllowNull()]$FailureRecord
    )

    $bundleError = $null
    if (-not $Succeeded) {
        try {
            Save-SmokeFailureBundle -Context $Context `
                -FailureRecord $FailureRecord
        }
        catch {
            $bundleError = $_
        }
    }
    $cleanupError = $null
    try {
        Stop-SmokeOwnedResources -Context $Context | Out-Null
    }
    catch {
        $cleanupError = $_
    }
    finally {
        Restore-SmokeEnvironment -Context $Context
    }
    if ($null -ne $bundleError) {
        Write-Warning (
            "Failure bundle collection was incomplete: " +
            ($bundleError | Out-String).Trim()
        )
    }

    if ($null -ne $cleanupError) {
        if (Test-Path -LiteralPath $Context.ManifestPath) {
            $manifest = Get-Content -LiteralPath $Context.ManifestPath -Raw |
                ConvertFrom-Json
            $manifest | Add-Member -NotePropertyName cleanup `
                -NotePropertyValue @{
                    path = [IO.Path]::GetFileName($Context.CleanupPath)
                    error = ($cleanupError | Out-String).Trim()
                } -Force
            $manifest | ConvertTo-Json -Depth 8 |
                Set-Content -LiteralPath $Context.ManifestPath -Encoding UTF8
        }
        if (-not $Succeeded) {
            Write-Warning (
                'Smoke cleanup also failed; preserving original test failure: ' +
                ($cleanupError | Out-String).Trim()
            )
        } else {
            Save-SmokeFailureBundle -Context $Context `
                -FailureRecord $cleanupError
            $Succeeded = $false
        }
    }
    elseif (Test-Path -LiteralPath $Context.ManifestPath) {
        $manifest = Get-Content -LiteralPath $Context.ManifestPath -Raw |
            ConvertFrom-Json
        $manifest | Add-Member -NotePropertyName cleanup `
            -NotePropertyValue @{
                path = [IO.Path]::GetFileName($Context.CleanupPath)
                orphan_free = $true
            } -Force
        $manifest | ConvertTo-Json -Depth 8 |
            Set-Content -LiteralPath $Context.ManifestPath -Encoding UTF8
    }

    if ($Succeeded) {
        $runPath = [IO.Path]::GetFullPath($Context.RunDirectory)
        $rootPath = [IO.Path]::GetFullPath($Context.OwnedRoot)
        $prefix = $rootPath.TrimEnd(
            [IO.Path]::DirectorySeparatorChar,
            [IO.Path]::AltDirectorySeparatorChar
        ) + [IO.Path]::DirectorySeparatorChar
        if (-not $runPath.StartsWith(
                $prefix, [StringComparison]::OrdinalIgnoreCase
            )) {
            throw "refusing to clean smoke path outside owned root: $runPath"
        }
        Remove-Item -LiteralPath $runPath -Recurse -Force
    } else {
        Write-Host "FAILURE BUNDLE $($Context.RunDirectory)"
    }
    if ($null -ne $cleanupError -and $null -eq $FailureRecord) {
        throw $cleanupError
    }
}

function New-SmokeRunContext {
    param(
        [Parameter(Mandatory = $true)][string]$Suite,
        [Parameter(Mandatory = $true)][string]$Executable,
        [string[]]$DeclaredEvidence = @(),
        [switch]$AllowPaneCapture
    )

    $executablePath = [IO.Path]::GetFullPath($Executable)
    if (-not (Test-Path -LiteralPath $executablePath)) {
        throw "AgenTerm executable not found: $executablePath"
    }
    $runId = '{0}-{1}-{2}' -f
        [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ'),
        $PID,
        ([Guid]::NewGuid().ToString('N').Substring(0, 8))
    $ownedRoot = [IO.Path]::GetFullPath(
        (Join-Path $PSScriptRoot '..\target\smoke\test-runs')
    )
    $runDirectory = Join-Path $ownedRoot "$Suite-$runId"
    $workspaceDirectory = Join-Path $runDirectory 'workspace'
    $settingsDirectory = Join-Path $runDirectory 'settings'
    $instanceDirectory = Join-Path $runDirectory 'instances'
    $evidenceDirectory = Join-Path $runDirectory 'evidence'
    $failureDirectory = Join-Path $runDirectory 'failure'
    foreach ($path in @(
        $workspaceDirectory, $settingsDirectory, $instanceDirectory,
        $evidenceDirectory
    )) {
        New-Item -ItemType Directory -Path $path -Force | Out-Null
    }
    $address = Get-SmokeLoopbackAddress
    $workspacePath = Join-Path $workspaceDirectory 'workspace.json'
    $settingsPath = Join-Path $settingsDirectory 'settings.json'
    $previousEnvironment = [ordered]@{
        AGENTERM_IPC_ADDRESS = $env:AGENTERM_IPC_ADDRESS
        AGENTERM_WORKSPACE_PATH = $env:AGENTERM_WORKSPACE_PATH
        AGENTERM_SETTINGS_PATH = $env:AGENTERM_SETTINGS_PATH
        AGENTERM_INSTANCE_DIR = $env:AGENTERM_INSTANCE_DIR
        AGENTERM_NO_ACTIVATE = $env:AGENTERM_NO_ACTIVATE
    }
    $env:AGENTERM_IPC_ADDRESS = $address
    $env:AGENTERM_WORKSPACE_PATH = $workspacePath
    $env:AGENTERM_SETTINGS_PATH = $settingsPath
    $env:AGENTERM_INSTANCE_DIR = $instanceDirectory
    $env:AGENTERM_NO_ACTIVATE = '1'

    $context = [pscustomobject]@{
        Suite = $Suite
        RunId = $runId
        StartedAtUtc = [DateTime]::UtcNow.ToString('o')
        Executable = $executablePath
        Address = $address
        OwnedRoot = $ownedRoot
        RunDirectory = $runDirectory
        WorkspaceDirectory = $workspaceDirectory
        SettingsDirectory = $settingsDirectory
        InstanceDirectory = $instanceDirectory
        EvidenceDirectory = $evidenceDirectory
        FailureDirectory = $failureDirectory
        WorkspacePath = $workspacePath
        SettingsPath = $settingsPath
        CommandLogPath = (Join-Path $runDirectory 'commands.log')
        EvidencePath = (Join-Path $evidenceDirectory 'emitted.txt')
        ManifestPath = (Join-Path $runDirectory 'manifest.json')
        CleanupPath = (Join-Path $runDirectory 'cleanup.json')
        CommandLogBytes = 0
        AllowPaneCapture = [bool]$AllowPaneCapture
        DeclaredEvidence = @($DeclaredEvidence)
        PreviousEnvironment = $previousEnvironment
        OwnedProcesses = [Collections.Generic.List[object]]::new()
        OwnedAddresses = [Collections.Generic.HashSet[string]]::new(
            [StringComparer]::Ordinal
        )
    }
    Register-SmokeOwnedAddress -Context $context -Address $address
    return $context
}
