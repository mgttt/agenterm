[CmdletBinding()]
param(
    [string]$OutputPath = (Join-Path $PSScriptRoot '..\evidence\local\windows-comparison.json'),
    [string]$NativeControlCenterPath = (Join-Path $PSScriptRoot '..\..\..\dist\agenterm-cc.exe'),
    [ValidateRange(1, 10)][int]$RepeatedStartupSamples = 3,
    [ValidateRange(30, 7200)][int]$BuildDeadlineSeconds = 1200,
    [ValidateRange(10, 600)][int]$MetadataDeadlineSeconds = 120,
    [ValidateRange(5, 120)][int]$ProbeDeadlineSeconds = 30,
    [ValidateRange(5, 120)][int]$SmokeDeadlineSeconds = 30,
    [ValidateRange(5, 600)][int]$ArchiveDeadlineSeconds = 120,
    [string]$RunId,
    [switch]$SelfCheck
)

$ErrorActionPreference = 'Stop'
$eventSchema = 'agenterm.webview-measurement-event/1'
$receiptSchema = 'agenterm.webview-comparison/1'
$workspace = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path

function ConvertTo-CompactJson {
    param([Parameter(Mandatory)]$Value)

    return ($Value | ConvertTo-Json -Depth 20 -Compress)
}

function Get-SourceFacts {
    param([Parameter(Mandatory)][string]$Workspace)

    $commit = (& git -C $Workspace rev-parse HEAD 2>$null)
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($commit)) {
        $commit = 'unavailable'
    }
    $porcelain = @(& git -C $Workspace status --porcelain --untracked-files=normal 2>$null)
    $dirty = if ($LASTEXITCODE -eq 0) { $porcelain.Count -gt 0 } else { $null }
    return [ordered]@{
        commit = ([string]$commit).Trim()
        dirty = $dirty
    }
}

function New-JournalContext {
    param(
        [Parameter(Mandatory)][string]$JournalPath,
        [Parameter(Mandatory)][string]$OutputPath,
        [Parameter(Mandatory)][string]$RunId,
        [Parameter(Mandatory)][string]$Platform,
        [Parameter(Mandatory)]$SourceFacts
    )

    $journalDirectory = Split-Path -Parent $JournalPath
    [System.IO.Directory]::CreateDirectory($journalDirectory) | Out-Null
    $stream = [System.IO.FileStream]::new(
        $JournalPath,
        [System.IO.FileMode]::CreateNew,
        [System.IO.FileAccess]::Write,
        [System.IO.FileShare]::Read
    )
    try {
        $stream.Flush($true)
    } finally {
        $stream.Dispose()
    }
    return [pscustomobject]@{
        JournalPath = $JournalPath
        OutputPath = $OutputPath
        RunId = $RunId
        Platform = $Platform
        SourceCommit = $SourceFacts.commit
        SourceDirty = $SourceFacts.dirty
        Sequence = 0
    }
}

function Assert-NewRunPaths {
    param(
        [Parameter(Mandatory)][string]$RunRoot,
        [Parameter(Mandatory)][string]$JournalPath
    )

    if (Test-Path -LiteralPath $RunRoot) {
        throw "run id collision: $RunRoot already exists"
    }
    if (Test-Path -LiteralPath $JournalPath) {
        throw "run id collision: $JournalPath already exists"
    }
}

function New-RunRootClaim {
    param(
        [Parameter(Mandatory)][string]$RunRoot,
        [Parameter(Mandatory)][string]$RunId
    )

    $runsDirectory = Split-Path -Parent $RunRoot
    [System.IO.Directory]::CreateDirectory($runsDirectory) | Out-Null
    $claimPath = "$RunRoot.claim"
    $encoding = [System.Text.UTF8Encoding]::new($false)
    $claimBytes = $encoding.GetBytes($RunId + "`n")
    $stream = [System.IO.FileStream]::new(
        $claimPath,
        [System.IO.FileMode]::CreateNew,
        [System.IO.FileAccess]::Write,
        [System.IO.FileShare]::Read
    )
    try {
        $stream.Write($claimBytes, 0, $claimBytes.Length)
        $stream.Flush($true)
    } finally {
        $stream.Dispose()
    }
    if (Test-Path -LiteralPath $RunRoot) {
        throw "run id collision: $RunRoot already exists"
    }
    [System.IO.Directory]::CreateDirectory($RunRoot) | Out-Null
    return $claimPath
}

function Write-JournalEvent {
    param(
        [Parameter(Mandatory)]$Context,
        [Parameter(Mandatory)][string]$Phase,
        [Parameter(Mandatory)][AllowEmptyString()][string]$Implementation,
        [Parameter(Mandatory)][ValidateSet('started', 'completed', 'failed', 'timed_out')][string]$Status,
        [Parameter(Mandatory)]$Facts
    )

    $Context.Sequence = [int]$Context.Sequence + 1
    $record = [ordered]@{
        schema = $eventSchema
        run_id = $Context.RunId
        sequence = $Context.Sequence
        observed_at_utc = [DateTimeOffset]::UtcNow.ToString('O')
        source_commit = $Context.SourceCommit
        source_tree_dirty = $Context.SourceDirty
        platform = $Context.Platform
        phase = $Phase
        implementation = $Implementation
        status = $Status
        facts = $Facts
    }
    $encoding = [System.Text.UTF8Encoding]::new($false)
    $bytes = $encoding.GetBytes((ConvertTo-CompactJson $record) + "`n")
    $stream = [System.IO.FileStream]::new(
        $Context.JournalPath,
        [System.IO.FileMode]::Append,
        [System.IO.FileAccess]::Write,
        [System.IO.FileShare]::Read
    )
    try {
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush($true)
    } finally {
        $stream.Dispose()
    }
    return $record
}

function Read-EventJournal {
    param([Parameter(Mandatory)][string]$JournalPath)

    $text = [System.IO.File]::ReadAllText($JournalPath, [System.Text.Encoding]::UTF8)
    $endsWithNewline = $text.EndsWith("`n", [System.StringComparison]::Ordinal)
    $lines = $text.Split("`n")
    $events = @()
    for ($index = 0; $index -lt $lines.Length; $index++) {
        $line = $lines[$index].TrimEnd("`r")
        if ([string]::IsNullOrWhiteSpace($line)) { continue }
        try {
            $event = $line | ConvertFrom-Json
        } catch {
            if ($index -eq ($lines.Length - 1) -and -not $endsWithNewline) {
                break
            }
            throw "invalid journal JSON at line $($index + 1): $($_.Exception.Message)"
        }
        $expectedSequence = $events.Count + 1
        if ([int]$event.sequence -ne $expectedSequence) {
            throw "journal sequence mismatch at line $($index + 1): expected $expectedSequence, got $($event.sequence)"
        }
        if ($event.schema -ne $eventSchema) {
            throw "journal schema mismatch at line $($index + 1): $($event.schema)"
        }
        $events += $event
    }
    return $events
}

function Fold-EventJournal {
    param([Parameter(Mandatory)][string]$JournalPath)

    $events = @(Read-EventJournal $JournalPath)
    if ($events.Count -eq 0) { throw 'cannot fold an empty event journal' }
    $terminalByPhase = [ordered]@{}
    foreach ($event in $events) {
        if ($event.status -ne 'started') {
            $terminalByPhase[$event.phase] = $event
        }
    }
    $terminalEvents = @($terminalByPhase.Values | Sort-Object sequence)
    $finalize = $terminalByPhase['finalize']
    $failed = @($terminalEvents | Where-Object { $_.status -in @('failed', 'timed_out') })
    $causalFailures = @($failed | Where-Object phase -ne 'finalize')
    $status = if ($null -ne $finalize -and $finalize.status -eq 'completed') {
        'complete'
    } elseif ($failed.Count -gt 0) {
        'incomplete'
    } else {
        'in_progress'
    }
    $reason = if ($status -eq 'complete') {
        'all_measurement_phases_completed'
    } elseif ($causalFailures.Count -gt 0) {
        "$($causalFailures[-1].phase)_$($causalFailures[-1].status)"
    } elseif ($failed.Count -gt 0) {
        "finalize_$($failed[-1].status)"
    } else {
        'run_interrupted_without_terminal_event'
    }
    $lastEvent = $events[-1]
    return [ordered]@{
        schema = $receiptSchema
        status = $status
        reason = $reason
        observed_at_utc = [DateTimeOffset]::UtcNow.ToString('O')
        run_id = $events[0].run_id
        platform = $events[0].platform
        source_commit = $events[0].source_commit
        source_tree_dirty = $events[0].source_tree_dirty
        active_renderer = 'native'
        journal_file = [System.IO.Path]::GetFileName($JournalPath)
        last_valid_sequence = $lastEvent.sequence
        in_progress_phase = if ($lastEvent.status -eq 'started') { $lastEvent.phase } else { $null }
        phase_receipts = $terminalEvents
        limitations = @(
            'empty-target build is a Rust target-cache measurement, not a cold OS or registry-download measurement',
            'load-complete is not first paint',
            'root-process peak working set excludes WebView child processes',
            'native Control Center startup/RSS remains owned by its isolated public journey',
            'licence expressions are metadata inventory and not a completed legal/SBOM review',
            'this Windows reference does not provide macOS or Linux renderer evidence'
        )
    }
}

function Write-AtomicJson {
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)]$Value
    )

    $directory = Split-Path -Parent $Path
    [System.IO.Directory]::CreateDirectory($directory) | Out-Null
    $temporaryPath = Join-Path $directory ('.' + [System.IO.Path]::GetFileName($Path) + '.tmp.' + [guid]::NewGuid().ToString('N'))
    $encoding = [System.Text.UTF8Encoding]::new($false)
    $bytes = $encoding.GetBytes(($Value | ConvertTo-Json -Depth 20) + "`n")
    $stream = [System.IO.FileStream]::new(
        $temporaryPath,
        [System.IO.FileMode]::CreateNew,
        [System.IO.FileAccess]::Write,
        [System.IO.FileShare]::None
    )
    try {
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush($true)
    } finally {
        $stream.Dispose()
    }
    try {
        [System.IO.File]::Move($temporaryPath, $Path, $true)
    } finally {
        if ([System.IO.File]::Exists($temporaryPath)) {
            [System.IO.File]::Delete($temporaryPath)
        }
    }
}

function Write-JournalCheckpoint {
    param([Parameter(Mandatory)]$Context)

    Write-AtomicJson $Context.OutputPath (Fold-EventJournal $Context.JournalPath)
}

function Write-TerminalEvent {
    param(
        [Parameter(Mandatory)]$Context,
        [Parameter(Mandatory)][string]$Phase,
        [Parameter(Mandatory)][AllowEmptyString()][string]$Implementation,
        [Parameter(Mandatory)][ValidateSet('completed', 'failed', 'timed_out')][string]$Status,
        [Parameter(Mandatory)]$Facts
    )

    Write-JournalEvent $Context $Phase $Implementation $Status $Facts | Out-Null
    Write-JournalCheckpoint $Context
}

function Get-TextTail {
    param(
        [AllowNull()][string]$Text,
        [int]$MaximumCharacters = 4096
    )

    if ([string]::IsNullOrEmpty($Text) -or $Text.Length -le $MaximumCharacters) { return $Text }
    return $Text.Substring($Text.Length - $MaximumCharacters)
}

function Invoke-DeadlineProcess {
    param(
        [Parameter(Mandatory)][string]$FileName,
        [Parameter(Mandatory)][string[]]$Arguments,
        [Parameter(Mandatory)][string]$WorkingDirectory,
        [Parameter(Mandatory)][int]$DeadlineSeconds
    )

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $FileName
    $startInfo.WorkingDirectory = $WorkingDirectory
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    foreach ($argument in $Arguments) { $startInfo.ArgumentList.Add($argument) }
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    try {
        $watch = [System.Diagnostics.Stopwatch]::StartNew()
        if (-not $process.Start()) { throw "failed to start $FileName" }
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        $timedOut = -not $process.WaitForExit($DeadlineSeconds * 1000)
        if ($timedOut) {
            try {
                if (-not $process.HasExited) { $process.Kill($true) }
            } catch [System.InvalidOperationException] {
                # The process exited between the deadline observation and kill.
            }
            $process.WaitForExit()
        } else {
            $process.WaitForExit()
        }
        $watch.Stop()
        $stdout = $stdoutTask.GetAwaiter().GetResult()
        $stderr = $stderrTask.GetAwaiter().GetResult()
        $peakWorkingSet = $null
        try {
            $process.Refresh()
            $peakWorkingSet = $process.PeakWorkingSet64
        } catch {
            $peakWorkingSet = $null
        }
        return [ordered]@{
            duration_ms = $watch.ElapsedMilliseconds
            exit_code = if ($timedOut) { $null } else { $process.ExitCode }
            timed_out = $timedOut
            stdout = $stdout
            stderr = $stderr
            root_process_peak_working_set_bytes = $peakWorkingSet
        }
    } finally {
        $process.Dispose()
    }
}

function Invoke-ProcessPhase {
    param(
        [Parameter(Mandatory)]$Context,
        [Parameter(Mandatory)][string]$Phase,
        [Parameter(Mandatory)][AllowEmptyString()][string]$Implementation,
        [Parameter(Mandatory)][string]$FileName,
        [Parameter(Mandatory)][string[]]$Arguments,
        [Parameter(Mandatory)][string]$WorkingDirectory,
        [Parameter(Mandatory)][int]$DeadlineSeconds,
        [Parameter(Mandatory)][scriptblock]$ResultFacts
    )

    Write-JournalEvent $Context $Phase $Implementation 'started' ([ordered]@{
        deadline_seconds = $DeadlineSeconds
        executable = [System.IO.Path]::GetFileName($FileName)
        arguments = $Arguments
    }) | Out-Null
    $terminalWritten = $false
    try {
        $result = Invoke-DeadlineProcess $FileName $Arguments $WorkingDirectory $DeadlineSeconds
        if ($result.timed_out) {
            Write-TerminalEvent $Context $Phase $Implementation 'timed_out' ([ordered]@{
                deadline_seconds = $DeadlineSeconds
                duration_ms = $result.duration_ms
                stdout_tail = Get-TextTail $result.stdout
                stderr_tail = Get-TextTail $result.stderr
            })
            $terminalWritten = $true
            throw [System.TimeoutException]::new("phase $Phase exceeded $DeadlineSeconds seconds")
        }
        if ($result.exit_code -ne 0) {
            Write-TerminalEvent $Context $Phase $Implementation 'failed' ([ordered]@{
                duration_ms = $result.duration_ms
                exit_code = $result.exit_code
                stdout_tail = Get-TextTail $result.stdout
                stderr_tail = Get-TextTail $result.stderr
            })
            $terminalWritten = $true
            throw "phase $Phase failed with exit code $($result.exit_code)"
        }
        $facts = [ordered]@{
            duration_ms = $result.duration_ms
            exit_code = $result.exit_code
        }
        $extraFacts = & $ResultFacts $result
        foreach ($entry in $extraFacts.GetEnumerator()) { $facts[$entry.Key] = $entry.Value }
        Write-TerminalEvent $Context $Phase $Implementation 'completed' $facts
        $terminalWritten = $true
        return $facts
    } catch {
        if (-not $terminalWritten) {
            Write-TerminalEvent $Context $Phase $Implementation 'failed' ([ordered]@{
                reason = $_.Exception.Message
            })
        }
        throw
    }
}

function Invoke-InProcessPhase {
    param(
        [Parameter(Mandatory)]$Context,
        [Parameter(Mandatory)][string]$Phase,
        [Parameter(Mandatory)][AllowEmptyString()][string]$Implementation,
        [Parameter(Mandatory)][int]$DeadlineSeconds,
        [Parameter(Mandatory)][scriptblock]$Action
    )

    Write-JournalEvent $Context $Phase $Implementation 'started' ([ordered]@{
        deadline_seconds = $DeadlineSeconds
    }) | Out-Null
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    try {
        $extraFacts = & $Action $watch $DeadlineSeconds
        $watch.Stop()
        if ($watch.Elapsed.TotalSeconds -gt $DeadlineSeconds) {
            throw [System.TimeoutException]::new("phase $Phase exceeded $DeadlineSeconds seconds")
        }
        $facts = [ordered]@{ duration_ms = $watch.ElapsedMilliseconds }
        foreach ($entry in $extraFacts.GetEnumerator()) { $facts[$entry.Key] = $entry.Value }
        Write-TerminalEvent $Context $Phase $Implementation 'completed' $facts
        return $facts
    } catch [System.TimeoutException] {
        $watch.Stop()
        Write-TerminalEvent $Context $Phase $Implementation 'timed_out' ([ordered]@{
            deadline_seconds = $DeadlineSeconds
            duration_ms = $watch.ElapsedMilliseconds
            reason = $_.Exception.Message
        })
        throw
    } catch {
        $watch.Stop()
        Write-TerminalEvent $Context $Phase $Implementation 'failed' ([ordered]@{
            duration_ms = $watch.ElapsedMilliseconds
            reason = $_.Exception.Message
        })
        throw
    }
}

function Get-FileFacts {
    param([Parameter(Mandatory)][string]$Path)

    return [ordered]@{
        bytes = (Get-Item -LiteralPath $Path).Length
        sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
    }
}

function New-ComparisonArchive {
    param(
        [Parameter(Mandatory)][string]$ArchivePath,
        [Parameter(Mandatory)][string[]]$Files,
        [Parameter(Mandatory)][System.Diagnostics.Stopwatch]$Watch,
        [Parameter(Mandatory)][int]$DeadlineSeconds
    )

    Add-Type -AssemblyName System.IO.Compression
    $fileStream = [System.IO.FileStream]::new(
        $ArchivePath,
        [System.IO.FileMode]::CreateNew,
        [System.IO.FileAccess]::ReadWrite,
        [System.IO.FileShare]::None
    )
    try {
        $archive = [System.IO.Compression.ZipArchive]::new(
            $fileStream,
            [System.IO.Compression.ZipArchiveMode]::Create,
            $true
        )
        try {
            foreach ($file in $Files) {
                if ($Watch.Elapsed.TotalSeconds -gt $DeadlineSeconds) {
                    throw [System.TimeoutException]::new('archive deadline exceeded')
                }
                $entry = $archive.CreateEntry(
                    [System.IO.Path]::GetFileName($file),
                    [System.IO.Compression.CompressionLevel]::Optimal
                )
                $input = [System.IO.File]::OpenRead($file)
                $output = $entry.Open()
                try {
                    $buffer = [byte[]]::new(65536)
                    while (($count = $input.Read($buffer, 0, $buffer.Length)) -gt 0) {
                        $output.Write($buffer, 0, $count)
                        if ($Watch.Elapsed.TotalSeconds -gt $DeadlineSeconds) {
                            throw [System.TimeoutException]::new('archive deadline exceeded')
                        }
                    }
                } finally {
                    $output.Dispose()
                    $input.Dispose()
                }
            }
        } finally {
            $archive.Dispose()
        }
        $fileStream.Flush($true)
    } catch {
        $fileStream.Dispose()
        if ([System.IO.File]::Exists($ArchivePath)) { [System.IO.File]::Delete($ArchivePath) }
        throw
    } finally {
        $fileStream.Dispose()
    }
    $facts = Get-FileFacts $ArchivePath
    $facts.contents = @($Files | ForEach-Object { [System.IO.Path]::GetFileName($_) } | Sort-Object)
    return $facts
}

function Get-InventoryFacts {
    param($ProcessResult)

    $metadata = $ProcessResult.stdout | ConvertFrom-Json
    $packages = @($metadata.packages | Sort-Object name, version)
    return [ordered]@{
        package_count = $packages.Count
        missing_license_count = @($packages | Where-Object { [string]::IsNullOrWhiteSpace($_.license) }).Count
        license_expressions = @($packages | ForEach-Object license | Where-Object { $_ } | Sort-Object -Unique)
    }
}

function Assert-SelfCheck {
    param(
        [Parameter(Mandatory)][bool]$Condition,
        [Parameter(Mandatory)][string]$Message
    )

    if (-not $Condition) { throw "self-check failed: $Message" }
}

function Invoke-MeasurementSelfCheck {
    $temporaryParent = [System.IO.Path]::GetTempPath()
    $temporaryRoot = Join-Path $temporaryParent ('agenterm-webview-measure-self-check-' + [guid]::NewGuid().ToString('N'))
    [System.IO.Directory]::CreateDirectory($temporaryRoot) | Out-Null
    try {
        $journal = Join-Path $temporaryRoot 'valid.events.jsonl'
        $output = Join-Path $temporaryRoot 'valid.json'
        $source = [ordered]@{ commit = 'self-check'; dirty = $false }
        $context = New-JournalContext $journal $output 'self-check-run' 'windows-x86_64' $source
        $collisionFailed = $false
        try {
            New-JournalContext $journal $output 'collision' 'windows-x86_64' $source | Out-Null
        } catch {
            $collisionFailed = $true
        }
        Assert-SelfCheck $collisionFailed 'existing journal collision must fail closed'

        $collidingRunRoot = Join-Path $temporaryRoot 'claimed-run'
        New-RunRootClaim $collidingRunRoot 'claimed-run' | Out-Null
        $runCollisionFailed = $false
        try {
            New-RunRootClaim $collidingRunRoot 'claimed-run' | Out-Null
        } catch {
            $runCollisionFailed = $true
        }
        Assert-SelfCheck $runCollisionFailed 'atomic run id claim collision must fail closed'

        Write-TerminalEvent $context 'initialize' 'shared' 'completed' ([ordered]@{ deadline_model = 'self-check' })
        Write-JournalEvent $context 'direct.clean_release_build' 'direct-wry' 'started' ([ordered]@{ deadline_seconds = 1200 }) | Out-Null
        Write-TerminalEvent $context 'direct.clean_release_build' 'direct-wry' 'completed' ([ordered]@{ duration_ms = 123 })
        Write-JournalEvent $context 'direct.smoke.0' 'direct-wry' 'started' ([ordered]@{ deadline_seconds = 30 }) | Out-Null
        Write-TerminalEvent $context 'direct.smoke.0' 'direct-wry' 'completed' ([ordered]@{
            duration_ms = 17
            receipt = [ordered]@{ status = 'loaded'; no_activate = $true }
        })
        $encoding = [System.Text.UTF8Encoding]::new($false)
        $truncated = $encoding.GetBytes('{"schema":')
        $stream = [System.IO.FileStream]::new($journal, [System.IO.FileMode]::Append, [System.IO.FileAccess]::Write, [System.IO.FileShare]::Read)
        try {
            $stream.Write($truncated, 0, $truncated.Length)
            $stream.Flush($true)
        } finally {
            $stream.Dispose()
        }
        $events = @(Read-EventJournal $journal)
        Assert-SelfCheck ($events.Count -eq 5) 'truncated final JSONL line must be ignored'
        $folded = Fold-EventJournal $journal
        $build = @($folded.phase_receipts | Where-Object phase -eq 'direct.clean_release_build')
        Assert-SelfCheck ($build.Count -eq 1 -and $build[0].facts.duration_ms -eq 123) 'fold must preserve completed build duration'
        $checkpoint = Get-Content -Raw $output | ConvertFrom-Json
        $checkpointBuild = @($checkpoint.phase_receipts | Where-Object phase -eq 'direct.clean_release_build')
        Assert-SelfCheck ($checkpointBuild.Count -eq 1 -and $checkpointBuild[0].facts.duration_ms -eq 123) 'atomic checkpoint must preserve completed duration'

        $badJournal = Join-Path $temporaryRoot 'bad-order.events.jsonl'
        $first = [ordered]@{ schema = $eventSchema; run_id = 'bad'; sequence = 1; phase = 'one'; status = 'completed' }
        $third = [ordered]@{ schema = $eventSchema; run_id = 'bad'; sequence = 3; phase = 'three'; status = 'completed' }
        [System.IO.File]::WriteAllText(
            $badJournal,
            (ConvertTo-CompactJson $first) + "`n" + (ConvertTo-CompactJson $third) + "`n",
            $encoding
        )
        $orderingFailed = $false
        try {
            Read-EventJournal $badJournal | Out-Null
        } catch {
            $orderingFailed = $true
        }
        Assert-SelfCheck $orderingFailed 'out-of-order event sequence must fail closed'

        $failureJournal = Join-Path $temporaryRoot 'failure.events.jsonl'
        $failureOutput = Join-Path $temporaryRoot 'failure.json'
        $failureContext = New-JournalContext $failureJournal $failureOutput 'failure-run' 'windows-x86_64' $source
        Write-TerminalEvent $failureContext 'initialize' 'shared' 'completed' ([ordered]@{})
        Write-TerminalEvent $failureContext 'tauri.clean_release_build' 'tauri-v2-reference' 'timed_out' ([ordered]@{ duration_ms = 1200000 })
        Write-TerminalEvent $failureContext 'finalize' 'shared' 'failed' ([ordered]@{ reason = 'phase timeout' })
        $failureFold = Fold-EventJournal $failureJournal
        Assert-SelfCheck (
            $failureFold.status -eq 'incomplete' -and
            $failureFold.reason -eq 'tauri.clean_release_build_timed_out'
        ) 'fold must preserve the causal phase timeout over the finalize wrapper failure'

        $pwshExecutable = Join-Path $PSHOME 'pwsh.exe'
        $processSuccess = Invoke-DeadlineProcess $pwshExecutable @(
            '-NoProfile',
            '-Command',
            '[Console]::Out.Write("ok")'
        ) $temporaryRoot 10
        Assert-SelfCheck (
            -not $processSuccess.timed_out -and
            $processSuccess.exit_code -eq 0 -and
            $processSuccess.stdout -eq 'ok'
        ) 'deadline process wrapper must preserve successful output'
        $processTimeout = Invoke-DeadlineProcess $pwshExecutable @(
            '-NoProfile',
            '-Command',
            '$gate = [System.Threading.ManualResetEvent]::new($false); $gate.WaitOne()'
        ) $temporaryRoot 1
        Assert-SelfCheck $processTimeout.timed_out 'deadline process wrapper must terminate its owned process tree'

        [ordered]@{
            schema = 'agenterm.webview-measurement-self-check/1'
            status = 'passed'
            checks = @(
                'journal_collision_fail_closed',
                'run_id_collision_fail_closed',
                'truncated_final_line_tolerated',
                'completed_duration_folded',
                'atomic_checkpoint_valid',
                'event_ordering_fail_closed',
                'causal_timeout_folded',
                'deadline_process_success',
                'owned_process_tree_timeout'
            )
        } | ConvertTo-Json -Depth 4
    } finally {
        $resolvedTemporaryParent = [System.IO.Path]::GetFullPath($temporaryParent)
        $resolvedTemporaryRoot = [System.IO.Path]::GetFullPath($temporaryRoot)
        if (
            $resolvedTemporaryRoot.StartsWith($resolvedTemporaryParent, [System.StringComparison]::OrdinalIgnoreCase) -and
            [System.IO.Path]::GetFileName($resolvedTemporaryRoot).StartsWith('agenterm-webview-measure-self-check-', [System.StringComparison]::Ordinal)
        ) {
            [System.IO.Directory]::Delete($resolvedTemporaryRoot, $true)
        }
    }
}

function Invoke-WebViewMeasurement {
    if (-not $IsWindows) { throw 'This measurement slice is Windows-only' }

    $effectiveRunId = if ([string]::IsNullOrWhiteSpace($RunId)) {
        [DateTimeOffset]::UtcNow.ToString('yyyyMMdd-HHmmss-fff')
    } else {
        $RunId
    }
    if ($effectiveRunId -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]{0,79}$') {
        throw 'RunId must be 1-80 characters using letters, digits, dot, underscore or hyphen'
    }
    $outputFullPath = [System.IO.Path]::GetFullPath($OutputPath)
    $outputDirectory = Split-Path -Parent $outputFullPath
    $outputStem = [System.IO.Path]::GetFileNameWithoutExtension($outputFullPath)
    $journalPath = Join-Path $outputDirectory "$outputStem.$effectiveRunId.events.jsonl"
    $localRoot = Join-Path $workspace 'evidence\local'
    $runRoot = Join-Path $localRoot "runs\$effectiveRunId"
    Assert-NewRunPaths $runRoot $journalPath

    $sourceFacts = Get-SourceFacts $workspace
    $claimPath = New-RunRootClaim $runRoot $effectiveRunId
    $context = New-JournalContext $journalPath $outputFullPath $effectiveRunId 'windows-x86_64' $sourceFacts
    $tauriWorkspace = Join-Path $workspace 'tauri-reference'
    $primaryManifest = Join-Path $workspace 'Cargo.toml'
    $tauriManifest = Join-Path $tauriWorkspace 'Cargo.toml'
    $directTarget = Join-Path $runRoot 'target-direct-wry'
    $tauriTarget = Join-Path $runRoot 'target-tauri'
    $launcher = Join-Path $directTarget 'release\agenterm-cc-web.exe'
    $directHost = Join-Path $directTarget 'release\agenterm-cc-web-direct-wry.exe'
    $tauriHost = Join-Path $tauriTarget 'release\agenterm-cc-web-tauri.exe'

    try {
        Write-TerminalEvent $context 'initialize' 'shared' 'completed' ([ordered]@{
            output_path = $outputFullPath
            journal_file = [System.IO.Path]::GetFileName($journalPath)
            run_claim_file = [System.IO.Path]::GetFileName($claimPath)
            run_root = $runRoot
            repeated_startup_samples = $RepeatedStartupSamples
            deadlines_seconds = [ordered]@{
                build = $BuildDeadlineSeconds
                metadata = $MetadataDeadlineSeconds
                probe = $ProbeDeadlineSeconds
                smoke = $SmokeDeadlineSeconds
                archive = $ArchiveDeadlineSeconds
            }
            runtime_policy = [ordered]@{
                system_runtime_only = $true
                runtime_download = $false
                fixed_runtime_bundled = $false
            }
        })

        Invoke-ProcessPhase $context 'toolchain.rustc' 'shared' 'rustc' @('--version') $workspace $ProbeDeadlineSeconds {
            param($result)
            [ordered]@{ version = $result.stdout.Trim() }
        } | Out-Null
        Invoke-ProcessPhase $context 'toolchain.cargo' 'shared' 'cargo' @('--version') $workspace $ProbeDeadlineSeconds {
            param($result)
            [ordered]@{ version = $result.stdout.Trim() }
        } | Out-Null

        $directBuildArguments = @('build', '--release', '--locked', '--manifest-path', $primaryManifest, '--target-dir', $directTarget, '-p', 'agenterm-cc-web-direct-wry')
        Invoke-ProcessPhase $context 'direct.clean_release_build' 'direct-wry' 'cargo' $directBuildArguments $workspace $BuildDeadlineSeconds {
            param($result)
            [ordered]@{ build_ms = $result.duration_ms; cache_state = 'empty_target' }
        } | Out-Null
        Invoke-ProcessPhase $context 'direct.warm_release_build' 'direct-wry' 'cargo' $directBuildArguments $workspace $BuildDeadlineSeconds {
            param($result)
            [ordered]@{ build_ms = $result.duration_ms; cache_state = 'warm_unchanged' }
        } | Out-Null
        $launcherBuildArguments = @('build', '--release', '--locked', '--manifest-path', $primaryManifest, '--target-dir', $directTarget, '-p', 'agenterm-cc-web')
        Invoke-ProcessPhase $context 'launcher.release_build' 'shared' 'cargo' $launcherBuildArguments $workspace $BuildDeadlineSeconds {
            param($result)
            [ordered]@{ build_ms = $result.duration_ms }
        } | Out-Null

        $tauriBuildArguments = @('build', '--release', '--locked', '--manifest-path', $tauriManifest, '--target-dir', $tauriTarget, '-p', 'agenterm-cc-web-tauri')
        Invoke-ProcessPhase $context 'tauri.clean_release_build' 'tauri-v2-reference' 'cargo' $tauriBuildArguments $tauriWorkspace $BuildDeadlineSeconds {
            param($result)
            [ordered]@{ build_ms = $result.duration_ms; cache_state = 'empty_target' }
        } | Out-Null
        Invoke-ProcessPhase $context 'tauri.warm_release_build' 'tauri-v2-reference' 'cargo' $tauriBuildArguments $tauriWorkspace $BuildDeadlineSeconds {
            param($result)
            [ordered]@{ build_ms = $result.duration_ms; cache_state = 'warm_unchanged' }
        } | Out-Null

        Invoke-InProcessPhase $context 'tauri.stage_fallback_sibling' 'tauri-v2-reference' $ArchiveDeadlineSeconds {
            param($watch, $deadline)
            $destination = Join-Path $directTarget 'release\agenterm-cc-web-tauri.exe'
            Copy-Item -LiteralPath $tauriHost -Destination $destination
            [ordered]@{ destination = [System.IO.Path]::GetFileName($destination) }
        } | Out-Null

        Invoke-InProcessPhase $context 'direct.artifact' 'direct-wry' $ArchiveDeadlineSeconds {
            param($watch, $deadline)
            Get-FileFacts $directHost
        } | Out-Null
        Invoke-InProcessPhase $context 'launcher.artifact' 'shared' $ArchiveDeadlineSeconds {
            param($watch, $deadline)
            Get-FileFacts $launcher
        } | Out-Null
        Invoke-InProcessPhase $context 'tauri.artifact' 'tauri-v2-reference' $ArchiveDeadlineSeconds {
            param($watch, $deadline)
            Get-FileFacts $tauriHost
        } | Out-Null

        Invoke-ProcessPhase $context 'direct.probe' 'direct-wry' $launcher @('--implementation', 'direct-wry', '--probe', '--no-activate') $directTarget $ProbeDeadlineSeconds {
            param($result)
            [ordered]@{ receipt = ($result.stdout | ConvertFrom-Json) }
        } | Out-Null
        Invoke-ProcessPhase $context 'tauri.probe' 'tauri-v2-reference' $launcher @('--implementation', 'tauri', '--probe', '--no-activate') $directTarget $ProbeDeadlineSeconds {
            param($result)
            [ordered]@{ receipt = ($result.stdout | ConvertFrom-Json) }
        } | Out-Null
        Invoke-ProcessPhase $context 'asset_manifest' 'shared' $launcher @('--asset-manifest') $directTarget $ProbeDeadlineSeconds {
            param($result)
            [ordered]@{ receipt = ($result.stdout | ConvertFrom-Json) }
        } | Out-Null

        for ($index = 0; $index -le $RepeatedStartupSamples; $index++) {
            Invoke-ProcessPhase $context "direct.smoke.$index" 'direct-wry' $directHost @('--smoke', '--no-activate') $directTarget $SmokeDeadlineSeconds {
                param($result)
                [ordered]@{
                    wall_ms = $result.duration_ms
                    root_process_peak_working_set_bytes = $result.root_process_peak_working_set_bytes
                    receipt = ($result.stdout | ConvertFrom-Json)
                    stderr_tail = Get-TextTail $result.stderr
                }
            } | Out-Null
            Invoke-ProcessPhase $context "tauri.smoke.$index" 'tauri-v2-reference' $tauriHost @('--smoke', '--no-activate') $tauriTarget $SmokeDeadlineSeconds {
                param($result)
                [ordered]@{
                    wall_ms = $result.duration_ms
                    root_process_peak_working_set_bytes = $result.root_process_peak_working_set_bytes
                    receipt = ($result.stdout | ConvertFrom-Json)
                    stderr_tail = Get-TextTail $result.stderr
                }
            } | Out-Null
        }

        $directArchivePath = Join-Path $runRoot 'direct-wry.zip'
        Invoke-InProcessPhase $context 'direct.archive' 'direct-wry' $ArchiveDeadlineSeconds {
            param($watch, $deadline)
            New-ComparisonArchive $directArchivePath @($launcher, $directHost) $watch $deadline
        } | Out-Null
        $tauriArchivePath = Join-Path $runRoot 'tauri-v2.zip'
        Invoke-InProcessPhase $context 'tauri.archive' 'tauri-v2-reference' $ArchiveDeadlineSeconds {
            param($watch, $deadline)
            New-ComparisonArchive $tauriArchivePath @($launcher, $tauriHost) $watch $deadline
        } | Out-Null

        if (Test-Path -LiteralPath $NativeControlCenterPath -PathType Leaf) {
            $nativePath = (Resolve-Path $NativeControlCenterPath).Path
            $nativeArchivePath = Join-Path $runRoot 'native-cc.zip'
            Invoke-InProcessPhase $context 'native.archive' 'native-control-center' $ArchiveDeadlineSeconds {
                param($watch, $deadline)
                $facts = New-ComparisonArchive $nativeArchivePath @($nativePath) $watch $deadline
                $facts.executable = Get-FileFacts $nativePath
                $facts.measurement_scope = 'artifact_only'
                return $facts
            } | Out-Null
        } else {
            Invoke-InProcessPhase $context 'native.archive' 'native-control-center' $ArchiveDeadlineSeconds {
                param($watch, $deadline)
                [ordered]@{
                    status = 'unavailable'
                    reason = 'native Control Center artifact was not found; startup/RSS were not inferred'
                }
            } | Out-Null
        }

        Invoke-ProcessPhase $context 'primary.inventory' 'direct-wry' 'cargo' @('metadata', '--locked', '--format-version', '1', '--manifest-path', $primaryManifest) $workspace $MetadataDeadlineSeconds {
            param($result)
            $facts = Get-InventoryFacts $result
            $facts.lock_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $workspace 'Cargo.lock')).Hash.ToLowerInvariant()
            return $facts
        } | Out-Null
        Invoke-ProcessPhase $context 'tauri.inventory' 'tauri-v2-reference' 'cargo' @('metadata', '--locked', '--format-version', '1', '--manifest-path', $tauriManifest) $tauriWorkspace $MetadataDeadlineSeconds {
            param($result)
            $facts = Get-InventoryFacts $result
            $facts.lock_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $tauriWorkspace 'Cargo.lock')).Hash.ToLowerInvariant()
            return $facts
        } | Out-Null

        Write-TerminalEvent $context 'finalize' 'shared' 'completed' ([ordered]@{
            decision = 'defer'
            active_renderer = 'native'
            reason = 'three-platform stability, isolation, rendering and resource evidence remain required'
        })
        Write-Output $outputFullPath
    } catch {
        $existingFinalize = @((Read-EventJournal $journalPath) | Where-Object phase -eq 'finalize')
        if ($existingFinalize.Count -eq 0) {
            Write-TerminalEvent $context 'finalize' 'shared' 'failed' ([ordered]@{
                reason = $_.Exception.Message
            })
        }
        throw
    }
}

if ($SelfCheck) {
    Invoke-MeasurementSelfCheck
    return
}

Invoke-WebViewMeasurement
