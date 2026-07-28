param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$policyPath = Join-Path $PSScriptRoot 'public-candidate-policy.ps1'
. $policyPath

function Assert-PublicPolicy {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if (-not $Condition) {
        throw $Message
    }
}

function Copy-PublicCandidateFixture {
    param([Parameter(Mandatory = $true)]$Candidate)

    return $Candidate | ConvertTo-Json -Depth 20 | ConvertFrom-Json
}

function New-PublicCandidateFixture {
    param(
        [ValidateSet('approved', 'missing', 'denied')]
        [string]$ApprovalStatus = 'approved'
    )

    $version = '0.1.8'
    $tag = 'v0.1.8'
    $commit = 'c' * 40
    $gateHash = 'a' * 64
    $artifactManifestHash = 'b' * 64
    $cargoHash = 'c' * 64
    $sbomHash = 'd' * 64
    $receiptHash = 'e' * 64
    $archiveHash = 'f' * 64
    $artifacts = @(
        [ordered]@{ name = 'agenterm.exe'; sha256 = '1' * 64 }
        [ordered]@{ name = 'agenterm-cli.exe'; sha256 = '2' * 64 }
        [ordered]@{ name = 'agenterm-mux.exe'; sha256 = '3' * 64 }
        [ordered]@{ name = 'agenterm-script.exe'; sha256 = '4' * 64 }
    )
    $approval = if ($ApprovalStatus -eq 'missing') {
        $null
    } else {
        [ordered]@{
            status = $ApprovalStatus
            version = $version
            tag = $tag
            commit = $commit
            archive_sha256 = $archiveHash
        }
    }

    return [ordered]@{
        schema_version = 1
        version = $version
        tag = $tag
        commit = $commit
        source = [ordered]@{
            clean = $true
            commit = $commit
        }
        expected = [ordered]@{
            gate_manifest_sha256 = $gateHash
            artifact_manifest_sha256 = $artifactManifestHash
            cargo_lock_sha256 = $cargoHash
            sbom_sha256 = $sbomHash
            artifacts = $artifacts
        }
        qualification = [ordered]@{
            status = 'complete'
            release = $true
            stress_included = $true
            all_gates_passed = $true
            commit = $commit
            receipt_sha256 = $receiptHash
            gate_manifest_sha256 = $gateHash
            artifact_manifest_sha256 = $artifactManifestHash
            cargo_lock_sha256 = $cargoHash
            sbom_sha256 = $sbomHash
            artifacts = $artifacts
        }
        package = [ordered]@{
            status = 'complete'
            no_rebuild = $true
            build_invocations = 0
            source_kind = 'qualified_bytes'
            qualified_commit = $commit
            qualification_receipt_sha256 = $receiptHash
            artifact_manifest_sha256 = $artifactManifestHash
            sbom_sha256 = $sbomHash
            artifacts = $artifacts
            archive_sha256 = $archiveHash
            observed_archive_sha256 = $archiveHash
        }
        rehearsal = [ordered]@{
            status = 'passed'
            commit = $commit
            tag = $tag
            archive_sha256 = $archiveHash
        }
        approval = $approval
    }
}

function Assert-PublicPolicyFailure {
    param(
        [Parameter(Mandatory = $true)]$Base,
        [Parameter(Mandatory = $true)][scriptblock]$Mutate,
        [Parameter(Mandatory = $true)][string]$ExpectedReason,
        [bool]$ExpectedRehearsalAllowed = $false
    )

    $candidate = Copy-PublicCandidateFixture -Candidate $Base
    & $Mutate $candidate
    $decision = Test-AgenTermPublicCandidatePolicy -Candidate $candidate
    Assert-PublicPolicy `
        -Condition ($decision.reason_codes -contains $ExpectedReason) `
        -Message (
            "expected reason $ExpectedReason, got: " +
            ($decision.reason_codes -join ', ')
        )
    Assert-PublicPolicy `
        -Condition (-not $decision.public_ready) `
        -Message "$ExpectedReason did not block public-ready"
    Assert-PublicPolicy `
        -Condition (-not $decision.publish_authorized) `
        -Message "$ExpectedReason did not block publication"
    Assert-PublicPolicy `
        -Condition (
            [bool]$decision.rehearsal_allowed -eq $ExpectedRehearsalAllowed
        ) `
        -Message "$ExpectedReason produced the wrong rehearsal decision"
}

Write-Host 'SELFTEST public candidate fully authorized success'
$approved = New-PublicCandidateFixture -ApprovalStatus 'approved'
$approvedDecision = Test-AgenTermPublicCandidatePolicy -Candidate $approved
Assert-PublicPolicy `
    -Condition (
        $approvedDecision.decision -eq 'publish_authorized' -and
        $approvedDecision.rehearsal_allowed -and
        $approvedDecision.public_ready -and
        $approvedDecision.publish_authorized -and
        $approvedDecision.reason_codes.Count -eq 0
    ) `
    -Message 'fully qualified and approved v0.1.8 was not publish-authorized'

Write-Host 'SELFTEST public-ready remains distinct from approval'
$unapproved = New-PublicCandidateFixture -ApprovalStatus 'missing'
$unapprovedDecision = Test-AgenTermPublicCandidatePolicy -Candidate $unapproved
Assert-PublicPolicy `
    -Condition (
        $unapprovedDecision.decision -eq 'public_ready' -and
        $unapprovedDecision.rehearsal_allowed -and
        $unapprovedDecision.public_ready -and
        -not $unapprovedDecision.publish_authorized -and
        $unapprovedDecision.reason_codes -contains 'approval.missing'
    ) `
    -Message 'v0.1.8 without approval was allowed to publish or lost readiness'

$base = New-PublicCandidateFixture -ApprovalStatus 'approved'

Write-Host 'SELFTEST fail-closed candidate and source identity'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.schema_version = 99 } `
    -ExpectedReason 'candidate.schema.unsupported'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.tag = 'v0.1.8-wrong' } `
    -ExpectedReason 'candidate.tag.mismatch'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.commit = 'short' } `
    -ExpectedReason 'candidate.commit.invalid'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.source.clean = $false } `
    -ExpectedReason 'source.dirty'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.source.commit = '9' * 40 } `
    -ExpectedReason 'source.commit.stale'

Write-Host 'SELFTEST fail-closed qualification receipt'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.qualification = $null } `
    -ExpectedReason 'qualification.receipt.missing'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.qualification.status = 'skipped' } `
    -ExpectedReason 'qualification.receipt.skipped'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.qualification.status = 'failed' } `
    -ExpectedReason 'qualification.receipt.incomplete'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.qualification.release = $false } `
    -ExpectedReason 'qualification.release-mode.required'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.qualification.stress_included = $false } `
    -ExpectedReason 'qualification.stress.required'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.qualification.all_gates_passed = $false } `
    -ExpectedReason 'qualification.gates.incomplete'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.qualification.commit = '9' * 40 } `
    -ExpectedReason 'qualification.commit.stale'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.qualification.receipt_sha256 = 'invalid' } `
    -ExpectedReason 'qualification.receipt-hash.invalid'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.qualification.gate_manifest_sha256 = '0' * 64 } `
    -ExpectedReason 'qualification.gate-manifest-hash.mismatch'
Assert-PublicPolicyFailure -Base $base `
    -Mutate {
        param($c)
        $c.qualification.artifact_manifest_sha256 = '0' * 64
    } `
    -ExpectedReason 'qualification.artifact-manifest-hash.mismatch'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.qualification.cargo_lock_sha256 = '0' * 64 } `
    -ExpectedReason 'qualification.cargo-lock-hash.mismatch'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.qualification.sbom_sha256 = '0' * 64 } `
    -ExpectedReason 'qualification.sbom-hash.mismatch'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.qualification.artifacts[0].sha256 = '0' * 64 } `
    -ExpectedReason 'qualification.artifacts.mismatch'

Write-Host 'SELFTEST fail-closed package provenance and tamper'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.package = $null } `
    -ExpectedReason 'package.provenance.missing'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.package.status = 'skipped' } `
    -ExpectedReason 'package.provenance.skipped'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.package.qualified_commit = '9' * 40 } `
    -ExpectedReason 'package.commit.stale'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.package.no_rebuild = $false } `
    -ExpectedReason 'package.rebuild.detected'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.package.build_invocations = 1 } `
    -ExpectedReason 'package.rebuild.detected'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.package.source_kind = 'rebuilt' } `
    -ExpectedReason 'package.rebuild.detected'
Assert-PublicPolicyFailure -Base $base `
    -Mutate {
        param($c)
        $c.package.qualification_receipt_sha256 = '0' * 64
    } `
    -ExpectedReason 'package.receipt-hash.mismatch'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.package.artifact_manifest_sha256 = '0' * 64 } `
    -ExpectedReason 'package.artifact-manifest-hash.mismatch'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.package.sbom_sha256 = '0' * 64 } `
    -ExpectedReason 'package.sbom-hash.mismatch'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.package.artifacts[2].sha256 = '0' * 64 } `
    -ExpectedReason 'package.artifacts.mismatch'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.package.observed_archive_sha256 = '0' * 64 } `
    -ExpectedReason 'package.archive.tampered'

Write-Host 'SELFTEST rehearsal is required but independently allowed'
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.rehearsal = $null } `
    -ExpectedReason 'rehearsal.missing' `
    -ExpectedRehearsalAllowed $true
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.rehearsal.status = 'skipped' } `
    -ExpectedReason 'rehearsal.skipped' `
    -ExpectedRehearsalAllowed $true
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.rehearsal.status = 'failed' } `
    -ExpectedReason 'rehearsal.failed' `
    -ExpectedRehearsalAllowed $true
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.rehearsal.commit = '9' * 40 } `
    -ExpectedReason 'rehearsal.commit.stale' `
    -ExpectedRehearsalAllowed $true
Assert-PublicPolicyFailure -Base $base `
    -Mutate { param($c) $c.rehearsal.archive_sha256 = '0' * 64 } `
    -ExpectedReason 'rehearsal.archive-hash.mismatch' `
    -ExpectedRehearsalAllowed $true

Write-Host 'SELFTEST v0.1.7 remains impossible to publish'
$internal = New-PublicCandidateFixture -ApprovalStatus 'approved'
$internal.version = '0.1.7'
$internal.tag = 'v0.1.7'
$internal.approval.version = '0.1.7'
$internal.approval.tag = 'v0.1.7'
$internal.rehearsal.tag = 'v0.1.7'
$internalDecision = Test-AgenTermPublicCandidatePolicy -Candidate $internal
Assert-PublicPolicy `
    -Condition (
        $internalDecision.reason_codes -contains 'version.internal-only' -and
        -not $internalDecision.rehearsal_allowed -and
        -not $internalDecision.public_ready -and
        -not $internalDecision.publish_authorized -and
        $internalDecision.decision -eq 'rejected'
    ) `
    -Message 'v0.1.7 internal-only policy was bypassed'

Write-Host 'SELFTEST approval is exact and cannot override readiness'
$staleApproval = New-PublicCandidateFixture -ApprovalStatus 'approved'
$staleApproval.approval.commit = '9' * 40
$staleApprovalDecision = Test-AgenTermPublicCandidatePolicy `
    -Candidate $staleApproval
Assert-PublicPolicy `
    -Condition (
        $staleApprovalDecision.public_ready -and
        -not $staleApprovalDecision.publish_authorized -and
        $staleApprovalDecision.reason_codes -contains 'approval.commit.stale'
    ) `
    -Message 'stale approval was not separated from public readiness'

$denied = New-PublicCandidateFixture -ApprovalStatus 'denied'
$deniedDecision = Test-AgenTermPublicCandidatePolicy -Candidate $denied
Assert-PublicPolicy `
    -Condition (
        $deniedDecision.public_ready -and
        -not $deniedDecision.publish_authorized -and
        $deniedDecision.reason_codes -contains 'approval.denied'
    ) `
    -Message 'denied approval was allowed to publish'

$approvedTamper = Copy-PublicCandidateFixture -Candidate (
    New-PublicCandidateFixture -ApprovalStatus 'approved'
)
$approvedTamper.qualification.artifacts[0].sha256 = '0' * 64
$approvedTamper.package.observed_archive_sha256 = '0' * 64
$approvedTamperDecision = Test-AgenTermPublicCandidatePolicy `
    -Candidate $approvedTamper
Assert-PublicPolicy `
    -Condition (
        -not $approvedTamperDecision.public_ready -and
        -not $approvedTamperDecision.publish_authorized -and
        $approvedTamperDecision.reason_codes -contains
            'qualification.artifacts.mismatch' -and
        $approvedTamperDecision.reason_codes -contains
            'package.archive.tampered'
    ) `
    -Message 'approval overrode receipt or archive integrity failure'

Write-Host 'SELFTEST policy source has no operational publication commands'
$policySource = Get-Content -LiteralPath $policyPath -Raw
foreach ($forbidden in @(
        '(?im)^\s*(git|cargo)\s',
        '(?im)^\s*&\s*(git|cargo)\b',
        '(?i)\bStart-Process\b',
        '(?i)\bInvoke-Web(Request|Method)\b',
        '(?i)\bNew-Item\s+.*tag\b'
    )) {
    Assert-PublicPolicy `
        -Condition ($policySource -notmatch $forbidden) `
        -Message "policy contains an operational side effect: $forbidden"
}

Write-Host 'PASS public candidate policy selftest'
