param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

. (Join-Path $PSScriptRoot 'JourneyManifest.ps1')

function Assert-JourneySelfTest {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if (-not $Condition) {
        throw $Message
    }
}

function Assert-JourneyThrows {
    param(
        [Parameter(Mandatory = $true)][scriptblock]$Action,
        [Parameter(Mandatory = $true)][string]$Message
    )

    $threw = $false
    try {
        & $Action
    }
    catch {
        $threw = $true
    }
    if (-not $threw) {
        throw $Message
    }
}

function New-SelfTestDirectory {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$Name
    )

    $path = Join-Path $Root $Name
    New-Item -ItemType Directory -Path $path -Force | Out-Null
    return $path
}

$temporaryBase = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$selfTestRoot = Join-Path $temporaryBase (
    "agenterm-journey-manifest-selftest-$PID-$([Guid]::NewGuid().ToString('N'))"
)
New-Item -ItemType Directory -Path $selfTestRoot | Out-Null

try {
    Write-Host 'SELFTEST journey manifest success path'
    $successRoot = New-SelfTestDirectory -Root $selfTestRoot -Name 'success'
    $successContext = New-JourneyManifestContext `
        -JourneyId 'daily.fleet' `
        -RunId 'success-1' `
        -OwnedRoot $successRoot `
        -BuildVersion '0.1.8' `
        -BuildCommit 'abcdef1' `
        -ExecutableSha256 ('a' * 64) `
        -ServerPid 4321 `
        -ServerAddress '127.0.0.1:48815' `
        -ServerEpoch 'epoch-success' `
        -TabId '@7'
    $before = New-JourneyCausalIdentity `
        -Context $successContext `
        -EventSequence 10 `
        -ModelSequence 20 `
        -RenderGeneration 30 `
        -LastPaintedSequence 29 `
        -OutputPosition 40
    Start-JourneyStep `
        -Context $successContext `
        -StepId '01.snapshot' `
        -CommandCategory 'observation' `
        -BeforeCausalIdentity $before
    $after = New-JourneyCausalIdentity `
        -Context $successContext `
        -EventSequence 11 `
        -ModelSequence 21 `
        -RenderGeneration 31 `
        -LastPaintedSequence 31 `
        -OutputPosition 42
    Complete-JourneyStep `
        -Context $successContext `
        -StepId '01.snapshot' `
        -ResultClass 'success' `
        -Message 'snapshot completed' `
        -AfterCausalIdentity $after | Out-Null
    $evidenceDirectory = New-SelfTestDirectory `
        -Root $successRoot -Name 'evidence'
    $snapshotPath = Join-Path $evidenceDirectory 'snapshot.json'
    [IO.File]::WriteAllText(
        $snapshotPath,
        '{"event_sequence":11}',
        [Text.UTF8Encoding]::new($false)
    )
    $artifact = Add-JourneyArtifact `
        -Context $successContext `
        -StepId '01.snapshot' `
        -Path $snapshotPath `
        -Kind 'snapshot' `
        -Redaction 'redacted'
    Set-JourneyCleanupResult `
        -Context $successContext `
        -Status 'success' `
        -OrphanFree $true `
        -OwnedCount 2
    $successManifest = Complete-JourneyManifest -Context $successContext
    $successPath = Join-Path $successRoot 'journey.json'
    Export-JourneyManifest `
        -Context $successContext -Path $successPath | Out-Null
    $successImported = Import-JourneyManifest -Path $successPath

    Assert-JourneySelfTest `
        -Condition ($successImported.result_class -eq 'success') `
        -Message 'success journey did not complete successfully'
    Assert-JourneySelfTest `
        -Condition (
            $successImported.steps.Count -eq 1 -and
            $successImported.steps[0].duration_ms -ge 0 -and
            $successImported.steps[0].before.event_sequence -eq 10 -and
            $successImported.steps[0].after.event_sequence -eq 11
        ) `
        -Message 'step timing or causal before/after identity was not retained'
    Assert-JourneySelfTest `
        -Condition (
            $successImported.identity.build.commit -eq 'abcdef1' -and
            $successImported.identity.server.pid -eq 4321 -and
            $successImported.identity.tab.id -eq '@7'
        ) `
        -Message 'build/server/tab identity was not retained'
    Assert-JourneySelfTest `
        -Condition (
            $artifact.path -eq 'evidence/snapshot.json' -and
            $artifact.bytes -gt 0 -and
            $artifact.sha256 -match '^[0-9a-f]{64}$' -and
            $successImported.cleanup.orphan_free
        ) `
        -Message 'artifact reference or orphan-free cleanup was not recorded'

    Write-Host 'SELFTEST journey manifest first-error and redaction path'
    $failureRoot = New-SelfTestDirectory -Root $selfTestRoot -Name 'failure'
    $failureContext = New-JourneyManifestContext `
        -JourneyId 'daily.fleet' `
        -RunId 'failure-1' `
        -OwnedRoot $failureRoot `
        -BuildVersion '0.1.8'
    $secretMessage = (
        'password=hunter2 ' +
        'token=plain-token ' +
        'ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890 ' +
        'https://alice:proxy-pass@example.test/path'
    )
    Start-JourneyStep `
        -Context $failureContext `
        -StepId '01.first-failure' `
        -CommandCategory 'cli'
    Complete-JourneyStep `
        -Context $failureContext `
        -StepId '01.first-failure' `
        -ResultClass 'failure' `
        -ErrorCode 'cli.failed' `
        -Message $secretMessage | Out-Null
    Start-JourneyStep `
        -Context $failureContext `
        -StepId '02.second-failure' `
        -CommandCategory 'assertion'
    Complete-JourneyStep `
        -Context $failureContext `
        -StepId '02.second-failure' `
        -ResultClass 'failure' `
        -ErrorCode 'assert.failed' `
        -Message 'password=second-secret' | Out-Null
    Set-JourneyCleanupResult `
        -Context $failureContext `
        -Status 'success' `
        -OrphanFree $true
    $failureManifest = Complete-JourneyManifest -Context $failureContext
    $failureJson = $failureManifest | ConvertTo-Json -Depth 20

    Assert-JourneySelfTest `
        -Condition (
            $failureManifest.result_class -eq 'failure' -and
            $failureManifest.first_failure.step_id -eq '01.first-failure' -and
            $failureManifest.first_failure.error_code -eq 'cli.failed'
        ) `
        -Message 'first failure was absent or overwritten by a later failure'
    foreach ($secret in @(
            'hunter2',
            'plain-token',
            'ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890',
            'alice',
            'proxy-pass',
            'second-secret'
        )) {
        Assert-JourneySelfTest `
            -Condition (-not $failureJson.Contains($secret)) `
            -Message "manifest leaked redacted content: $secret"
    }
    Assert-JourneySelfTest `
        -Condition ($failureJson.Contains('<redacted>')) `
        -Message 'manifest did not expose that sensitive text was redacted'

    Write-Host 'SELFTEST journey manifest size and path boundaries'
    $boundaryRoot = New-SelfTestDirectory -Root $selfTestRoot -Name 'boundary'
    $boundaryContext = New-JourneyManifestContext `
        -JourneyId 'daily.fleet' `
        -RunId 'boundary-1' `
        -OwnedRoot $boundaryRoot
    Start-JourneyStep `
        -Context $boundaryContext `
        -StepId '01.bounded' `
        -CommandCategory 'assertion'
    Complete-JourneyStep `
        -Context $boundaryContext `
        -StepId '01.bounded' `
        -ResultClass 'success' `
        -Message ([string]::new('x', 8192)) | Out-Null
    $boundedStep = Get-JourneyCompletedStep `
        -Context $boundaryContext -StepId '01.bounded'
    Assert-JourneySelfTest `
        -Condition (
            (Get-JourneyUtf8ByteCount -Text $boundedStep.message) -le 2048 -and
            $boundedStep.message.EndsWith('<truncated>')
        ) `
        -Message 'step message did not honor its UTF-8 size boundary'

    for ($index = 0; $index -lt 9; $index++) {
        $artifactPath = Join-Path $boundaryRoot "artifact-$index.txt"
        [IO.File]::WriteAllText(
            $artifactPath,
            "artifact-$index",
            [Text.UTF8Encoding]::new($false)
        )
        if ($index -lt 8) {
            Add-JourneyArtifact `
                -Context $boundaryContext `
                -StepId '01.bounded' `
                -Path $artifactPath `
                -Kind 'log' `
                -Redaction 'metadata_only' | Out-Null
        } else {
            Assert-JourneyThrows `
                -Action {
                    Add-JourneyArtifact `
                        -Context $boundaryContext `
                        -StepId '01.bounded' `
                        -Path $artifactPath `
                        -Kind 'log' `
                        -Redaction 'metadata_only' | Out-Null
                } `
                -Message 'ninth artifact reference was not rejected'
        }
    }
    $oversizedPath = Join-Path $boundaryRoot 'oversized.bin'
    [IO.File]::WriteAllBytes(
        $oversizedPath,
        [byte[]]::new(1MB + 1)
    )
    $outsidePath = Join-Path $selfTestRoot 'outside.txt'
    [IO.File]::WriteAllText(
        $outsidePath,
        'outside',
        [Text.UTF8Encoding]::new($false)
    )
    $sizeContext = New-JourneyManifestContext `
        -JourneyId 'daily.fleet' `
        -RunId 'size-1' `
        -OwnedRoot $boundaryRoot
    Start-JourneyStep `
        -Context $sizeContext `
        -StepId '01.artifact' `
        -CommandCategory 'assertion'
    Complete-JourneyStep `
        -Context $sizeContext `
        -StepId '01.artifact' `
        -ResultClass 'success' | Out-Null
    Assert-JourneyThrows `
        -Action {
            Add-JourneyArtifact `
                -Context $sizeContext `
                -StepId '01.artifact' `
                -Path $oversizedPath `
                -Kind 'other' `
                -Redaction 'metadata_only' | Out-Null
        } `
        -Message 'oversized artifact was not rejected'
    Assert-JourneyThrows `
        -Action {
            Add-JourneyArtifact `
                -Context $sizeContext `
                -StepId '01.artifact' `
                -Path $outsidePath `
                -Kind 'other' `
                -Redaction 'metadata_only' | Out-Null
        } `
        -Message 'artifact outside the owned root was not rejected'
    Set-JourneyCleanupResult `
        -Context $boundaryContext `
        -Status 'success' `
        -OrphanFree $true
    Complete-JourneyManifest -Context $boundaryContext | Out-Null
    Set-JourneyCleanupResult `
        -Context $sizeContext `
        -Status 'success' `
        -OrphanFree $true
    Complete-JourneyManifest -Context $sizeContext | Out-Null
    $boundaryJson = $boundaryContext.Manifest | ConvertTo-Json -Depth 20
    Assert-JourneySelfTest `
        -Condition (-not $boundaryJson.Contains($boundaryRoot)) `
        -Message 'manifest exposed the absolute journey-owned root'

    Write-Host 'SELFTEST journey manifest cleanup failure truth'
    $orphanRoot = New-SelfTestDirectory -Root $selfTestRoot -Name 'orphan'
    $orphanContext = New-JourneyManifestContext `
        -JourneyId 'daily.fleet' `
        -RunId 'orphan-1' `
        -OwnedRoot $orphanRoot
    Set-JourneyCleanupResult `
        -Context $orphanContext `
        -Status 'failure' `
        -OrphanFree $false `
        -OwnedCount 3 `
        -ForcedCount 1 `
        -RemainingProcesses 1 `
        -RemainingWindows 1 `
        -RemainingRegistrations 1 `
        -ErrorCode 'cleanup.orphaned'
    $orphanManifest = Complete-JourneyManifest -Context $orphanContext
    Assert-JourneySelfTest `
        -Condition (
            $orphanManifest.result_class -eq 'failure' -and
            -not $orphanManifest.cleanup.orphan_free -and
            $orphanManifest.cleanup.remaining_processes -eq 1 -and
            $orphanManifest.cleanup.remaining_windows -eq 1 -and
            $orphanManifest.cleanup.remaining_registrations -eq 1
        ) `
        -Message 'cleanup/orphan failure was not truthfully retained'

    Write-Host 'SELFTEST journey manifest invalid schema rejection'
    $invalid = Get-Content -LiteralPath $successPath -Raw | ConvertFrom-Json
    $invalid.schema_version = 99
    $invalidPath = Join-Path $successRoot 'invalid-schema.json'
    [IO.File]::WriteAllText(
        $invalidPath,
        ($invalid | ConvertTo-Json -Depth 20),
        [Text.UTF8Encoding]::new($false)
    )
    Assert-JourneyThrows `
        -Action {
            Import-JourneyManifest -Path $invalidPath | Out-Null
        } `
        -Message 'unsupported journey manifest schema was not rejected'

    Write-Host 'PASS journey manifest selftest'
}
finally {
    $resolvedRoot = [IO.Path]::GetFullPath($selfTestRoot)
    $temporaryPrefix = $temporaryBase.TrimEnd(
        [IO.Path]::DirectorySeparatorChar,
        [IO.Path]::AltDirectorySeparatorChar
    ) + [IO.Path]::DirectorySeparatorChar
    if (-not $resolvedRoot.StartsWith(
            $temporaryPrefix, [StringComparison]::OrdinalIgnoreCase
        ) -or
        [IO.Path]::GetFileName($resolvedRoot) -notlike
            'agenterm-journey-manifest-selftest-*') {
        throw "refusing to clean unexpected selftest path: $resolvedRoot"
    }
    if (Test-Path -LiteralPath $resolvedRoot) {
        Remove-Item -LiteralPath $resolvedRoot -Recurse -Force
    }
}
