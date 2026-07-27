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

function Stop-SmokeOwnedServer {
    param([Parameter(Mandatory = $true)]$Context)

    $savedPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $output = @(
            & $Context.Executable --address $Context.Address kill-server 2>&1
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
    $text = "exit_code=$exitCode`n" + ($output -join "`n")
    $text = Limit-SmokeText -Text $text -MaximumBytes 8192
    [IO.File]::WriteAllText(
        (Join-Path $Context.RunDirectory 'cleanup-server.txt'),
        $text,
        [Text.UTF8Encoding]::new($false)
    )
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
    try {
        Stop-SmokeOwnedServer -Context $Context
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
    $evidenceDirectory = Join-Path $runDirectory 'evidence'
    $failureDirectory = Join-Path $runDirectory 'failure'
    foreach ($path in @(
        $workspaceDirectory, $settingsDirectory, $evidenceDirectory
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
        AGENTERM_NO_ACTIVATE = $env:AGENTERM_NO_ACTIVATE
    }
    $env:AGENTERM_IPC_ADDRESS = $address
    $env:AGENTERM_WORKSPACE_PATH = $workspacePath
    $env:AGENTERM_SETTINGS_PATH = $settingsPath
    $env:AGENTERM_NO_ACTIVATE = '1'

    return [pscustomobject]@{
        Suite = $Suite
        RunId = $runId
        StartedAtUtc = [DateTime]::UtcNow.ToString('o')
        Executable = $executablePath
        Address = $address
        OwnedRoot = $ownedRoot
        RunDirectory = $runDirectory
        WorkspaceDirectory = $workspaceDirectory
        SettingsDirectory = $settingsDirectory
        EvidenceDirectory = $evidenceDirectory
        FailureDirectory = $failureDirectory
        WorkspacePath = $workspacePath
        SettingsPath = $settingsPath
        CommandLogPath = (Join-Path $runDirectory 'commands.log')
        EvidencePath = (Join-Path $evidenceDirectory 'emitted.txt')
        ManifestPath = (Join-Path $runDirectory 'manifest.json')
        CommandLogBytes = 0
        AllowPaneCapture = [bool]$AllowPaneCapture
        DeclaredEvidence = @($DeclaredEvidence)
        PreviousEnvironment = $previousEnvironment
    }
}
