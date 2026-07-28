param(
    [string]$CliExe = (Join-Path $PSScriptRoot '..\dist\agenterm-cli.exe')
)

$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
. (Join-Path $PSScriptRoot 'qualification.ps1')
. (Join-Path $root 'tests\TestHarness.ps1')

function Assert-Rejected {
    param(
        [Parameter(Mandatory = $true)][scriptblock]$Action,
        [Parameter(Mandatory = $true)][string]$Pattern
    )

    try {
        & $Action
    }
    catch {
        $message = [string]$_.Exception.Message
        if ($message -notmatch $Pattern) {
            throw "Expected rejection matching '$Pattern', got: $_"
        }
        return
    }
    throw "Expected rejection matching '$Pattern', but the action succeeded."
}

function Remove-QualificationScratch {
    param([Parameter(Mandatory = $true)][string]$Path)

    $resolved = [IO.Path]::GetFullPath($Path)
    $ownedRoots = @(
        'target\qualification'
        'target\smoke\test-runs'
    ) | ForEach-Object {
        [IO.Path]::GetFullPath((Join-Path $root $_)).TrimEnd(
            [IO.Path]::DirectorySeparatorChar,
            [IO.Path]::AltDirectorySeparatorChar
        ) + [IO.Path]::DirectorySeparatorChar
    }
    if (-not @($ownedRoots | Where-Object {
                $resolved.StartsWith(
                    $_, [StringComparison]::OrdinalIgnoreCase
                )
            })) {
        throw "Refusing to remove qualification path outside owned root: $resolved"
    }
    if (Test-Path -LiteralPath $resolved) {
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}

$missingManifest = Join-Path $root 'target\qualification\missing-gates.json'
Assert-Rejected -Pattern 'manifest does not exist' -Action {
    Read-AgenTermQualificationManifest -Path $missingManifest
}

$gateManifest = Join-Path $root 'scripts\qualification-gates.json'
foreach ($rejectedStatus in @('skipped', 'failed')) {
    $resultContext = New-AgenTermQualificationContext `
        -ManifestPath $gateManifest -Release $false -StressIncluded $false
    foreach ($gate in @($resultContext.Manifest.required_gates)) {
        $status = if ($gate.id -eq 'rustfmt') {
            $rejectedStatus
        } else {
            'passed'
        }
        $output = @($gate.evidence | ForEach-Object { "EVIDENCE $_" })
        Add-AgenTermQualificationResult -Context $resultContext `
            -GateId $gate.id -Status $status -DurationMs 0 -Output $output
    }
    Assert-Rejected -Pattern 'is not passed' -Action {
        Assert-AgenTermQualificationResults -Context $resultContext
    }
}

$noStressContext = New-AgenTermQualificationContext `
    -ManifestPath $gateManifest -Release $false -StressIncluded $false
foreach ($gate in @($noStressContext.Manifest.required_gates)) {
    Add-AgenTermQualificationResult -Context $noStressContext `
        -GateId $gate.id -Status passed -DurationMs 0 `
        -Output @($gate.evidence | ForEach-Object { "EVIDENCE $_" })
}
Assert-Rejected -Pattern 'requires the explicit stress gate' -Action {
    Assert-AgenTermQualificationResults -Context $noStressContext
}

$scratch = Join-Path $root (
    'target\qualification\selftest-' + [Guid]::NewGuid().ToString('N')
)
New-Item -ItemType Directory -Path $scratch -Force | Out-Null
try {
    $metadata = Get-Content -LiteralPath (
        Join-Path $root 'dist\agenterm.json'
    ) -Raw | ConvertFrom-Json
    $head = (& git -C $root rev-parse HEAD).Trim().ToLowerInvariant()
    $metadata.git_commit = $head
    $metadata.git_dirty = $true
    $metadata.cargo_lock_sha256 = Get-QualificationSha256 -Path (
        Join-Path $root 'Cargo.lock'
    )
    $metadata.artifact_manifest_sha256 = ('0' * 64)
    foreach ($entry in @($metadata.executables)) {
        $entry.sha256 = Get-QualificationSha256 -Path (
            Join-Path (Join-Path $root 'dist') $entry.name
        )
    }
    $metadataPath = Join-Path $scratch 'mismatched-agenterm.json'
    $metadata | ConvertTo-Json -Depth 6 |
        Set-Content -LiteralPath $metadataPath -Encoding UTF8
    Assert-Rejected -Pattern 'artifact manifest hash mismatch' -Action {
        Get-AgenTermQualificationProvenance `
            -RepoRoot $root `
            -BuildMetadataPath $metadataPath `
            -ArtifactManifestPath (Join-Path $root 'scripts\artifacts.json') `
            -StagedDirectory (Join-Path $root 'dist') `
            -RequireClean $false
    }

    $context = New-SmokeRunContext -Suite 'qualification-selftest' `
        -Executable $CliExe
    $succeeded = $false
    $failure = $null
    try {
        Invoke-SmokeCli -Context $context `
            -Arguments @('__qualification_invalid_command__') | Out-Null
    }
    catch {
        $failure = $_
    }
    finally {
        Complete-SmokeRun -Context $context -Succeeded $succeeded `
            -FailureRecord $failure
    }
    if ($null -eq $failure) {
        throw 'CLI failure-bundle injection did not fail as expected.'
    }
    if (-not (Test-Path -LiteralPath $context.ManifestPath)) {
        throw 'CLI failure-bundle injection did not emit a manifest.'
    }
    $bundle = Get-Content -LiteralPath $context.ManifestPath -Raw |
        ConvertFrom-Json
    if ($bundle.privacy.pane_capture -ne 'disabled for this suite' -or
        $bundle.diagnostics -contains 'capture-pane.txt' -or
        -not (Test-Path -LiteralPath $context.CommandLogPath)) {
        throw 'CLI failure bundle violated its bounded diagnostics contract.'
    }
    Remove-QualificationScratch -Path $context.RunDirectory
}
finally {
    Remove-QualificationScratch -Path $scratch
}

Write-Host 'PASS: qualification fail-closed self-test'
exit 0
