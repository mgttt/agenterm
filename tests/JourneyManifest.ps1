$script:JourneyManifestSchemaVersion = 1
$script:JourneyManifestKind = 'agenterm.journey-manifest'
$script:JourneyManifestMaximumBytes = 256KB
$script:JourneyMaximumSteps = 128
$script:JourneyMaximumArtifactsPerStep = 8
$script:JourneyMaximumArtifactBytes = 1MB
$script:JourneyMaximumArtifactPathBytes = 240
$script:JourneyMaximumMessageBytes = 2048

function Get-JourneyUtf8ByteCount {
    param([AllowNull()][string]$Text)

    if ($null -eq $Text) {
        return 0
    }
    return [Text.UTF8Encoding]::new($false).GetByteCount($Text)
}

function Limit-JourneyText {
    param(
        [AllowNull()][string]$Text,
        [Parameter(Mandatory = $true)][ValidateRange(1, 1048576)]
        [int]$MaximumBytes
    )

    if ($null -eq $Text) {
        return $null
    }
    $encoding = [Text.UTF8Encoding]::new($false)
    $bytes = $encoding.GetBytes($Text)
    if ($bytes.Length -le $MaximumBytes) {
        return $Text
    }

    $marker = '<truncated>'
    $markerBytes = $encoding.GetByteCount($marker)
    if ($markerBytes -ge $MaximumBytes) {
        return ''
    }
    $prefixLimit = $MaximumBytes - $markerBytes
    while ($prefixLimit -gt 0) {
        try {
            $prefix = [Text.UTF8Encoding]::new($false, $true).GetString(
                $bytes, 0, $prefixLimit
            )
            return "$prefix$marker"
        }
        catch {
            $prefixLimit--
        }
    }
    return $marker
}

function Protect-JourneyText {
    param([AllowNull()][string]$Text)

    if ($null -eq $Text) {
        return $null
    }
    $safe = $Text
    $safe = [regex]::Replace(
        $safe,
        '(?i)\b(authorization|token|password|secret|api[_-]?key)\s*[:=]\s*[^\s,;]+',
        '$1=<redacted>'
    )
    $safe = [regex]::Replace(
        $safe,
        '(?i)\bgh[pousr]_[A-Za-z0-9_]{20,}\b',
        '<redacted-token>'
    )
    $safe = [regex]::Replace(
        $safe,
        '(?i)(https?://)[^/\s:@]+:[^@\s/]+@',
        '$1<redacted>@'
    )
    return Limit-JourneyText -Text $safe `
        -MaximumBytes $script:JourneyMaximumMessageBytes
}

function Assert-JourneyId {
    param(
        [Parameter(Mandatory = $true)][string]$Value,
        [Parameter(Mandatory = $true)][string]$Field
    )

    if ($Value -notmatch '^[a-z0-9][a-z0-9._-]{0,63}$') {
        throw "$Field must match ^[a-z0-9][a-z0-9._-]{0,63}$"
    }
}

function Copy-JourneyIdentity {
    param([Parameter(Mandatory = $true)]$Identity)

    return [ordered]@{
        build = [ordered]@{
            version = $Identity.build.version
            commit = $Identity.build.commit
            executable_sha256 = $Identity.build.executable_sha256
        }
        server = [ordered]@{
            pid = $Identity.server.pid
            address = $Identity.server.address
            epoch = $Identity.server.epoch
        }
        tab = [ordered]@{
            id = $Identity.tab.id
        }
    }
}

function New-JourneyManifestContext {
    param(
        [Parameter(Mandatory = $true)][string]$JourneyId,
        [Parameter(Mandatory = $true)][string]$RunId,
        [Parameter(Mandatory = $true)][string]$OwnedRoot,
        [AllowNull()][string]$BuildVersion,
        [AllowNull()][string]$BuildCommit,
        [AllowNull()][string]$ExecutableSha256,
        [AllowNull()][Nullable[int]]$ServerPid,
        [AllowNull()][string]$ServerAddress,
        [AllowNull()][string]$ServerEpoch,
        [AllowNull()][string]$TabId
    )

    Assert-JourneyId -Value $JourneyId -Field 'journey_id'
    Assert-JourneyId -Value $RunId -Field 'run_id'
    $root = [IO.Path]::GetFullPath($OwnedRoot)
    if (-not (Test-Path -LiteralPath $root -PathType Container)) {
        throw "owned root does not exist: $root"
    }
    if (-not [string]::IsNullOrWhiteSpace($BuildCommit) -and
        $BuildCommit -notmatch '^[0-9a-fA-F]{7,64}$') {
        throw 'build commit must be a 7-64 character hexadecimal identity'
    }
    if (-not [string]::IsNullOrWhiteSpace($ExecutableSha256) -and
        $ExecutableSha256 -notmatch '^[0-9a-fA-F]{64}$') {
        throw 'executable SHA-256 must contain exactly 64 hexadecimal characters'
    }
    if ($null -ne $ServerPid -and [int]$ServerPid -le 0) {
        throw 'server PID must be positive when present'
    }
    if (-not [string]::IsNullOrWhiteSpace($TabId) -and
        $TabId -notmatch '^@[1-9][0-9]*$') {
        throw 'tab identity must use stable @N form'
    }
    if ([string]::IsNullOrWhiteSpace($BuildVersion)) { $BuildVersion = $null }
    if ([string]::IsNullOrWhiteSpace($BuildCommit)) { $BuildCommit = $null }
    if ([string]::IsNullOrWhiteSpace($ExecutableSha256)) {
        $ExecutableSha256 = $null
    }
    if ([string]::IsNullOrWhiteSpace($ServerAddress)) { $ServerAddress = $null }
    if ([string]::IsNullOrWhiteSpace($ServerEpoch)) { $ServerEpoch = $null }
    if ([string]::IsNullOrWhiteSpace($TabId)) { $TabId = $null }

    $identity = [ordered]@{
        build = [ordered]@{
            version = Protect-JourneyText -Text $BuildVersion
            commit = $BuildCommit
            executable_sha256 = $ExecutableSha256
        }
        server = [ordered]@{
            pid = if ($null -eq $ServerPid) { $null } else { [int]$ServerPid }
            address = Protect-JourneyText -Text $ServerAddress
            epoch = Protect-JourneyText -Text $ServerEpoch
        }
        tab = [ordered]@{
            id = $TabId
        }
    }
    $manifest = [ordered]@{
        schema_version = $script:JourneyManifestSchemaVersion
        kind = $script:JourneyManifestKind
        journey_id = $JourneyId
        run_id = $RunId
        started_at_utc = [DateTime]::UtcNow.ToString('o')
        completed_at_utc = $null
        result_class = 'running'
        identity = $identity
        steps = [Collections.ArrayList]::new()
        first_failure = $null
        cleanup = [ordered]@{
            status = 'not_run'
            orphan_free = $null
            owned_count = 0
            forced_count = 0
            remaining_processes = 0
            remaining_windows = 0
            remaining_registrations = 0
            error_code = $null
        }
        boundaries = [ordered]@{
            arguments = 'not recorded'
            stdout_stderr = 'artifact references only; never inlined'
            message_redaction = 'credential patterns redacted and UTF-8 bounded'
            artifact_paths = 'relative to owned root'
            maximum_steps = $script:JourneyMaximumSteps
            maximum_artifacts_per_step = $script:JourneyMaximumArtifactsPerStep
            maximum_artifact_bytes = $script:JourneyMaximumArtifactBytes
            maximum_manifest_bytes = $script:JourneyManifestMaximumBytes
        }
    }

    return [pscustomobject]@{
        Manifest = $manifest
        OwnedRoot = $root
        ActiveSteps = [Collections.Generic.Dictionary[string, object]]::new(
            [StringComparer]::Ordinal
        )
    }
}

function New-JourneyCausalIdentity {
    param(
        [Parameter(Mandatory = $true)]$Context,
        [AllowNull()][string]$ServerEpoch,
        [AllowNull()][Nullable[long]]$EventSequence,
        [AllowNull()][Nullable[long]]$ModelSequence,
        [AllowNull()][Nullable[long]]$RenderGeneration,
        [AllowNull()][Nullable[long]]$LastPaintedSequence,
        [AllowNull()][Nullable[long]]$OutputPosition
    )

    $identity = Copy-JourneyIdentity -Identity $Context.Manifest.identity
    if (-not [string]::IsNullOrWhiteSpace($ServerEpoch)) {
        $identity.server.epoch = Protect-JourneyText -Text $ServerEpoch
    }
    return [ordered]@{
        identity = $identity
        event_sequence = if ($null -eq $EventSequence) {
            $null
        } else {
            [int64]$EventSequence
        }
        model_sequence = if ($null -eq $ModelSequence) {
            $null
        } else {
            [int64]$ModelSequence
        }
        render_generation = if ($null -eq $RenderGeneration) {
            $null
        } else {
            [int64]$RenderGeneration
        }
        last_painted_sequence = if ($null -eq $LastPaintedSequence) {
            $null
        } else {
            [int64]$LastPaintedSequence
        }
        output_position = if ($null -eq $OutputPosition) {
            $null
        } else {
            [int64]$OutputPosition
        }
    }
}

function Start-JourneyStep {
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)][string]$StepId,
        [Parameter(Mandatory = $true)]
        [ValidateSet(
            'cli', 'gui', 'script', 'terminal', 'control', 'observation',
            'assertion', 'cleanup'
        )]
        [string]$CommandCategory,
        [AllowNull()]$BeforeCausalIdentity
    )

    Assert-JourneyId -Value $StepId -Field 'step id'
    if ($Context.Manifest.result_class -ne 'running') {
        throw 'cannot start a step after the journey has completed'
    }
    if ($Context.ActiveSteps.Count -ne 0) {
        throw 'journey steps are serial; complete the active step first'
    }
    if ($Context.Manifest.steps.Count -ge $script:JourneyMaximumSteps) {
        throw "journey exceeds the $script:JourneyMaximumSteps step limit"
    }
    if ($Context.ActiveSteps.ContainsKey($StepId) -or
        $null -ne ($Context.Manifest.steps | Where-Object { $_.id -eq $StepId })) {
        throw "duplicate journey step id: $StepId"
    }
    if ($null -eq $BeforeCausalIdentity) {
        $BeforeCausalIdentity = New-JourneyCausalIdentity -Context $Context
    }

    $Context.ActiveSteps.Add(
        $StepId,
        [pscustomobject]@{
            StartedAtUtc = [DateTime]::UtcNow.ToString('o')
            Stopwatch = [Diagnostics.Stopwatch]::StartNew()
            CommandCategory = $CommandCategory
            Before = $BeforeCausalIdentity
        }
    )
}

function Complete-JourneyStep {
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)][string]$StepId,
        [Parameter(Mandatory = $true)]
        [ValidateSet('success', 'expected_failure', 'failure', 'skipped')]
        [string]$ResultClass,
        [AllowNull()][string]$ErrorCode,
        [AllowNull()][string]$Message,
        [AllowNull()]$AfterCausalIdentity
    )

    if (-not $Context.ActiveSteps.ContainsKey($StepId)) {
        throw "journey step is not active: $StepId"
    }
    $active = $Context.ActiveSteps[$StepId]
    $active.Stopwatch.Stop()
    if ($null -eq $AfterCausalIdentity) {
        $AfterCausalIdentity = New-JourneyCausalIdentity -Context $Context
    }
    $normalizedErrorCode = if ([string]::IsNullOrWhiteSpace($ErrorCode)) {
        $null
    } else {
        $ErrorCode
    }
    if ($ResultClass -eq 'failure' -and $null -eq $normalizedErrorCode) {
        throw 'a failed step requires a stable error code'
    }
    if ($null -ne $normalizedErrorCode -and
        $normalizedErrorCode -notmatch '^[a-z0-9][a-z0-9._-]{0,63}$') {
        throw 'error code must be a stable lowercase identifier'
    }

    $step = [ordered]@{
        ordinal = $Context.Manifest.steps.Count + 1
        id = $StepId
        command_category = $active.CommandCategory
        started_at_utc = $active.StartedAtUtc
        duration_ms = [Math]::Max(0, [int64]$active.Stopwatch.ElapsedMilliseconds)
        result_class = $ResultClass
        error_code = $normalizedErrorCode
        message = Protect-JourneyText -Text $Message
        before = $active.Before
        after = $AfterCausalIdentity
        artifacts = [Collections.ArrayList]::new()
    }
    $Context.Manifest.steps.Add($step) | Out-Null
    $Context.ActiveSteps.Remove($StepId) | Out-Null

    if ($ResultClass -eq 'failure' -and
        $null -eq $Context.Manifest.first_failure) {
        $Context.Manifest.first_failure = [ordered]@{
            step_id = $StepId
            ordinal = $step.ordinal
            error_code = $normalizedErrorCode
            message = $step.message
        }
    }
    return $step
}

function Get-JourneyCompletedStep {
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)][string]$StepId
    )

    $matches = @($Context.Manifest.steps | Where-Object { $_.id -eq $StepId })
    if ($matches.Count -ne 1) {
        throw "journey step is not completed: $StepId"
    }
    return $matches[0]
}

function Add-JourneyArtifact {
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)][string]$StepId,
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)]
        [ValidateSet(
            'snapshot', 'screenshot', 'capture', 'events', 'log', 'cleanup',
            'other'
        )]
        [string]$Kind,
        [Parameter(Mandatory = $true)]
        [ValidateSet('public', 'redacted', 'metadata_only')]
        [string]$Redaction
    )

    $step = Get-JourneyCompletedStep -Context $Context -StepId $StepId
    if ($step.artifacts.Count -ge $script:JourneyMaximumArtifactsPerStep) {
        throw (
            "step $StepId exceeds the " +
            "$script:JourneyMaximumArtifactsPerStep artifact limit"
        )
    }
    $candidate = if ([IO.Path]::IsPathRooted($Path)) {
        [IO.Path]::GetFullPath($Path)
    } else {
        [IO.Path]::GetFullPath((Join-Path $Context.OwnedRoot $Path))
    }
    $rootPrefix = $Context.OwnedRoot.TrimEnd(
        [IO.Path]::DirectorySeparatorChar,
        [IO.Path]::AltDirectorySeparatorChar
    ) + [IO.Path]::DirectorySeparatorChar
    if (-not $candidate.StartsWith(
            $rootPrefix, [StringComparison]::OrdinalIgnoreCase
        )) {
        throw "artifact is outside the journey-owned root: $candidate"
    }
    if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) {
        throw "artifact does not exist or is not a file: $candidate"
    }
    $file = Get-Item -LiteralPath $candidate
    if ($file.Length -gt $script:JourneyMaximumArtifactBytes) {
        throw (
            "artifact exceeds $script:JourneyMaximumArtifactBytes bytes: " +
            "$candidate"
        )
    }
    $relative = $candidate.Substring($rootPrefix.Length).Replace('\', '/')
    if ((Get-JourneyUtf8ByteCount -Text $relative) -gt
        $script:JourneyMaximumArtifactPathBytes) {
        throw 'artifact relative path exceeds its UTF-8 boundary'
    }
    $record = [ordered]@{
        kind = $Kind
        path = $relative
        bytes = [int64]$file.Length
        sha256 = (Get-FileHash -LiteralPath $candidate -Algorithm SHA256).Hash.ToLowerInvariant()
        redaction = $Redaction
    }
    $step.artifacts.Add($record) | Out-Null
    return $record
}

function Set-JourneyCleanupResult {
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)]
        [ValidateSet('success', 'failure')]
        [string]$Status,
        [Parameter(Mandatory = $true)][bool]$OrphanFree,
        [ValidateRange(0, 2147483647)][int]$OwnedCount = 0,
        [ValidateRange(0, 2147483647)][int]$ForcedCount = 0,
        [ValidateRange(0, 2147483647)][int]$RemainingProcesses = 0,
        [ValidateRange(0, 2147483647)][int]$RemainingWindows = 0,
        [ValidateRange(0, 2147483647)][int]$RemainingRegistrations = 0,
        [AllowNull()][string]$ErrorCode
    )

    if ($Context.Manifest.cleanup.status -ne 'not_run') {
        throw 'journey cleanup result was already recorded'
    }
    $hasRemaining = (
        $RemainingProcesses -gt 0 -or
        $RemainingWindows -gt 0 -or
        $RemainingRegistrations -gt 0
    )
    if ($Status -eq 'success' -and (-not $OrphanFree -or $hasRemaining)) {
        throw 'successful cleanup must be orphan-free with no remaining resources'
    }
    if ($OrphanFree -and $hasRemaining) {
        throw 'orphan-free cleanup cannot report remaining resources'
    }
    $normalizedErrorCode = if ([string]::IsNullOrWhiteSpace($ErrorCode)) {
        $null
    } else {
        $ErrorCode
    }
    if ($Status -eq 'failure' -and $null -eq $normalizedErrorCode) {
        throw 'failed cleanup requires a stable error code'
    }
    if ($null -ne $normalizedErrorCode -and
        $normalizedErrorCode -notmatch '^[a-z0-9][a-z0-9._-]{0,63}$') {
        throw 'cleanup error code must be a stable lowercase identifier'
    }

    $Context.Manifest.cleanup = [ordered]@{
        status = $Status
        orphan_free = $OrphanFree
        owned_count = $OwnedCount
        forced_count = $ForcedCount
        remaining_processes = $RemainingProcesses
        remaining_windows = $RemainingWindows
        remaining_registrations = $RemainingRegistrations
        error_code = $normalizedErrorCode
    }
}

function Assert-JourneyManifest {
    param([Parameter(Mandatory = $true)]$Manifest)

    if ([int]$Manifest.schema_version -ne $script:JourneyManifestSchemaVersion) {
        throw "unsupported journey manifest schema: $($Manifest.schema_version)"
    }
    if ([string]$Manifest.kind -ne $script:JourneyManifestKind) {
        throw "invalid journey manifest kind: $($Manifest.kind)"
    }
    Assert-JourneyId -Value ([string]$Manifest.journey_id) -Field 'journey_id'
    Assert-JourneyId -Value ([string]$Manifest.run_id) -Field 'run_id'
    if ([string]$Manifest.result_class -notin @(
            'running', 'success', 'failure'
        )) {
        throw "invalid journey result class: $($Manifest.result_class)"
    }
    $steps = @($Manifest.steps)
    if ($steps.Count -gt $script:JourneyMaximumSteps) {
        throw 'journey manifest exceeds its step boundary'
    }
    $seen = @{}
    for ($position = 0; $position -lt $steps.Count; $position++) {
        $step = $steps[$position]
        Assert-JourneyId -Value ([string]$step.id) -Field 'step id'
        if ($seen.ContainsKey([string]$step.id)) {
            throw "duplicate journey step id: $($step.id)"
        }
        $seen[[string]$step.id] = $true
        if ([int]$step.ordinal -ne $position + 1) {
            throw "invalid journey step ordinal for $($step.id)"
        }
        if ([int64]$step.duration_ms -lt 0) {
            throw "negative journey step duration for $($step.id)"
        }
        if ([string]$step.command_category -notin @(
                'cli', 'gui', 'script', 'terminal', 'control', 'observation',
                'assertion', 'cleanup'
            )) {
            throw "invalid command category for $($step.id)"
        }
        if ([string]$step.result_class -notin @(
                'success', 'expected_failure', 'failure', 'skipped'
            )) {
            throw "invalid step result class for $($step.id)"
        }
        $artifacts = @($step.artifacts)
        if ($artifacts.Count -gt $script:JourneyMaximumArtifactsPerStep) {
            throw "step $($step.id) exceeds its artifact boundary"
        }
        foreach ($artifact in $artifacts) {
            if ([IO.Path]::IsPathRooted([string]$artifact.path) -or
                [string]$artifact.path -match '(^|/)\.\.(/|$)') {
                throw "artifact path is not a safe relative reference: $($artifact.path)"
            }
            if ([int64]$artifact.bytes -lt 0 -or
                [int64]$artifact.bytes -gt $script:JourneyMaximumArtifactBytes) {
                throw "artifact size is outside its boundary: $($artifact.path)"
            }
            if ([string]$artifact.sha256 -notmatch '^[0-9a-f]{64}$') {
                throw "artifact hash is invalid: $($artifact.path)"
            }
            if ([string]$artifact.redaction -notin @(
                    'public', 'redacted', 'metadata_only'
                )) {
                throw "artifact redaction class is invalid: $($artifact.path)"
            }
        }
    }
    if ([string]$Manifest.result_class -ne 'running' -and
        [string]$Manifest.cleanup.status -eq 'not_run') {
        throw 'completed journey manifest has no cleanup result'
    }
    if ([string]$Manifest.cleanup.status -eq 'success' -and
        -not [bool]$Manifest.cleanup.orphan_free) {
        throw 'successful cleanup is not orphan-free'
    }
    if ([bool]$Manifest.cleanup.orphan_free -and (
            [int]$Manifest.cleanup.remaining_processes -ne 0 -or
            [int]$Manifest.cleanup.remaining_windows -ne 0 -or
            [int]$Manifest.cleanup.remaining_registrations -ne 0
        )) {
        throw 'orphan-free cleanup reports remaining resources'
    }
    return $true
}

function Complete-JourneyManifest {
    param([Parameter(Mandatory = $true)]$Context)

    if ($Context.ActiveSteps.Count -ne 0) {
        throw 'cannot complete a journey while a step is active'
    }
    if ($Context.Manifest.cleanup.status -eq 'not_run') {
        throw 'cannot complete a journey before cleanup is recorded'
    }
    $Context.Manifest.completed_at_utc = [DateTime]::UtcNow.ToString('o')
    $Context.Manifest.result_class = if (
        $null -eq $Context.Manifest.first_failure -and
        $Context.Manifest.cleanup.status -eq 'success' -and
        $Context.Manifest.cleanup.orphan_free
    ) {
        'success'
    } else {
        'failure'
    }
    Assert-JourneyManifest -Manifest $Context.Manifest | Out-Null
    return $Context.Manifest
}

function Export-JourneyManifest {
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)][string]$Path
    )

    Assert-JourneyManifest -Manifest $Context.Manifest | Out-Null
    $json = $Context.Manifest | ConvertTo-Json -Depth 20
    if ((Get-JourneyUtf8ByteCount -Text $json) -gt
        $script:JourneyManifestMaximumBytes) {
        throw 'journey manifest exceeds its serialized UTF-8 boundary'
    }
    $destination = [IO.Path]::GetFullPath($Path)
    $parent = Split-Path -Parent $destination
    if (-not (Test-Path -LiteralPath $parent -PathType Container)) {
        throw "journey manifest parent does not exist: $parent"
    }
    $temporary = "$destination.tmp-$PID-$([Guid]::NewGuid().ToString('N'))"
    try {
        [IO.File]::WriteAllText(
            $temporary,
            $json,
            [Text.UTF8Encoding]::new($false)
        )
        Move-Item -LiteralPath $temporary -Destination $destination -Force
    }
    finally {
        if (Test-Path -LiteralPath $temporary) {
            Remove-Item -LiteralPath $temporary -Force
        }
    }
    return $destination
}

function Import-JourneyManifest {
    param([Parameter(Mandatory = $true)][string]$Path)

    $file = Get-Item -LiteralPath $Path -ErrorAction Stop
    if ($file.Length -gt $script:JourneyManifestMaximumBytes) {
        throw 'journey manifest file exceeds its byte boundary'
    }
    $manifest = Get-Content -LiteralPath $file.FullName -Raw |
        ConvertFrom-Json
    Assert-JourneyManifest -Manifest $manifest | Out-Null
    return $manifest
}
