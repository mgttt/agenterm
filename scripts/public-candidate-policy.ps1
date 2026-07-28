$script:PublicCandidatePolicySchemaVersion = 1
$script:PublicCandidatePolicyKind = 'agenterm.public-candidate-decision'

function Get-PublicCandidateProperty {
    param(
        [AllowNull()]$Object,
        [Parameter(Mandatory = $true)][string]$Name
    )

    if ($null -eq $Object) {
        return $null
    }
    if ($Object -is [Collections.IDictionary]) {
        if ($Object.Contains($Name)) {
            return $Object[$Name]
        }
        return $null
    }
    $property = $Object.PSObject.Properties[$Name]
    if ($null -eq $property) {
        return $null
    }
    return $property.Value
}

function Add-PublicCandidateReason {
    param(
        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [Collections.Generic.List[object]]$Reasons,
        [Parameter(Mandatory = $true)][string]$Code,
        [Parameter(Mandatory = $true)]
        [ValidateSet('readiness', 'authorization')]
        [string]$Scope,
        [Parameter(Mandatory = $true)][string]$Message
    )

    if ($null -ne ($Reasons | Where-Object { $_.code -eq $Code })) {
        return
    }
    $Reasons.Add([ordered]@{
        code = $Code
        scope = $Scope
        message = $Message
    })
}

function Test-PublicCandidateHash {
    param([AllowNull()]$Value)

    return (
        $null -ne $Value -and
        [string]$Value -cmatch '^[0-9a-f]{64}$'
    )
}

function Compare-PublicCandidateArtifacts {
    param(
        [AllowNull()]$Expected,
        [AllowNull()]$Actual,
        [Parameter(Mandatory = $true)][string]$Prefix,
        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [Collections.Generic.List[object]]$Reasons
    )

    $expectedEntries = @($Expected)
    $actualEntries = @($Actual)
    if ($expectedEntries.Count -eq 0) {
        Add-PublicCandidateReason -Reasons $Reasons `
            -Code 'expected.artifacts.missing' -Scope 'readiness' `
            -Message 'The expected public artifact set is absent.'
        return $false
    }
    if ($actualEntries.Count -eq 0) {
        Add-PublicCandidateReason -Reasons $Reasons `
            -Code "$Prefix.artifacts.missing" -Scope 'readiness' `
            -Message 'The candidate artifact hash set is absent.'
        return $false
    }

    $expectedMap = @{}
    foreach ($entry in $expectedEntries) {
        $name = [string](Get-PublicCandidateProperty -Object $entry -Name 'name')
        $hash = [string](Get-PublicCandidateProperty -Object $entry -Name 'sha256')
        if ([string]::IsNullOrWhiteSpace($name) -or
            -not (Test-PublicCandidateHash -Value $hash) -or
            $expectedMap.ContainsKey($name)) {
            Add-PublicCandidateReason -Reasons $Reasons `
                -Code 'expected.artifacts.invalid' -Scope 'readiness' `
                -Message 'The expected artifact set has an invalid or duplicate entry.'
            return $false
        }
        $expectedMap[$name] = $hash
    }

    $actualMap = @{}
    foreach ($entry in $actualEntries) {
        $name = [string](Get-PublicCandidateProperty -Object $entry -Name 'name')
        $hash = [string](Get-PublicCandidateProperty -Object $entry -Name 'sha256')
        if ([string]::IsNullOrWhiteSpace($name) -or
            -not (Test-PublicCandidateHash -Value $hash) -or
            $actualMap.ContainsKey($name)) {
            Add-PublicCandidateReason -Reasons $Reasons `
                -Code "$Prefix.artifacts.invalid" -Scope 'readiness' `
                -Message 'The candidate artifact set has an invalid or duplicate entry.'
            return $false
        }
        $actualMap[$name] = $hash
    }
    if ($expectedMap.Count -ne $actualMap.Count) {
        Add-PublicCandidateReason -Reasons $Reasons `
            -Code "$Prefix.artifacts.mismatch" -Scope 'readiness' `
            -Message 'The candidate artifact names do not exactly match the expected set.'
        return $false
    }
    foreach ($name in $expectedMap.Keys) {
        if (-not $actualMap.ContainsKey($name) -or
            $actualMap[$name] -cne $expectedMap[$name]) {
            Add-PublicCandidateReason -Reasons $Reasons `
                -Code "$Prefix.artifacts.mismatch" -Scope 'readiness' `
                -Message 'The candidate artifact hashes do not exactly match the expected set.'
            return $false
        }
    }
    return $true
}

function Test-PublicCandidateExactHash {
    param(
        [AllowNull()]$ExpectedValue,
        [AllowNull()]$ActualValue,
        [Parameter(Mandatory = $true)][string]$MissingCode,
        [Parameter(Mandatory = $true)][string]$InvalidCode,
        [Parameter(Mandatory = $true)][string]$MismatchCode,
        [Parameter(Mandatory = $true)][string]$Label,
        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [Collections.Generic.List[object]]$Reasons
    )

    if ($null -eq $ActualValue -or
        [string]::IsNullOrWhiteSpace([string]$ActualValue)) {
        Add-PublicCandidateReason -Reasons $Reasons `
            -Code $MissingCode -Scope 'readiness' `
            -Message "$Label is missing."
        return $false
    }
    if (-not (Test-PublicCandidateHash -Value $ExpectedValue) -or
        -not (Test-PublicCandidateHash -Value $ActualValue)) {
        Add-PublicCandidateReason -Reasons $Reasons `
            -Code $InvalidCode -Scope 'readiness' `
            -Message "$Label is not a canonical lowercase SHA-256 value."
        return $false
    }
    if ([string]$ExpectedValue -cne [string]$ActualValue) {
        Add-PublicCandidateReason -Reasons $Reasons `
            -Code $MismatchCode -Scope 'readiness' `
            -Message "$Label does not match the exact expected bytes."
        return $false
    }
    return $true
}

function Test-AgenTermPublicCandidatePolicy {
    param(
        [Parameter(Mandatory = $true)]$Candidate,
        [string]$ExpectedVersion = '0.1.8'
    )

    $reasons = [Collections.Generic.List[object]]::new()
    $version = [string](Get-PublicCandidateProperty -Object $Candidate -Name 'version')
    $tag = [string](Get-PublicCandidateProperty -Object $Candidate -Name 'tag')
    $commit = [string](Get-PublicCandidateProperty -Object $Candidate -Name 'commit')
    $schemaVersion = Get-PublicCandidateProperty -Object $Candidate -Name 'schema_version'

    if ($schemaVersion -ne $script:PublicCandidatePolicySchemaVersion) {
        Add-PublicCandidateReason -Reasons $reasons `
            -Code 'candidate.schema.unsupported' -Scope 'readiness' `
            -Message 'The candidate decision input schema is missing or unsupported.'
    }
    if ($version -eq '0.1.7') {
        Add-PublicCandidateReason -Reasons $reasons `
            -Code 'version.internal-only' -Scope 'readiness' `
            -Message 'AgenTerm 0.1.7 is internal-only and can never be published.'
    } elseif ($version -ne $ExpectedVersion) {
        Add-PublicCandidateReason -Reasons $reasons `
            -Code 'version.unexpected' -Scope 'readiness' `
            -Message "The candidate version does not match $ExpectedVersion."
    }
    if ($version -notmatch '^\d+\.\d+\.\d+([+-][0-9A-Za-z.-]+)?$') {
        Add-PublicCandidateReason -Reasons $reasons `
            -Code 'version.invalid' -Scope 'readiness' `
            -Message 'The candidate version is not valid semantic versioning.'
    }
    if ($tag -ne "v$version") {
        Add-PublicCandidateReason -Reasons $reasons `
            -Code 'candidate.tag.mismatch' -Scope 'readiness' `
            -Message 'The candidate tag does not exactly match its version.'
    }
    if ($commit -notmatch '^[0-9a-f]{40}$') {
        Add-PublicCandidateReason -Reasons $reasons `
            -Code 'candidate.commit.invalid' -Scope 'readiness' `
            -Message 'The candidate commit is not a full lowercase commit identity.'
    }

    $source = Get-PublicCandidateProperty -Object $Candidate -Name 'source'
    $sourceClean = Get-PublicCandidateProperty -Object $source -Name 'clean'
    $sourceCommit = [string](
        Get-PublicCandidateProperty -Object $source -Name 'commit'
    )
    if ($null -eq $sourceClean) {
        Add-PublicCandidateReason -Reasons $reasons `
            -Code 'source.clean.missing' -Scope 'readiness' `
            -Message 'The source cleanliness observation is missing.'
    } elseif ($sourceClean -isnot [bool] -or -not [bool]$sourceClean) {
        Add-PublicCandidateReason -Reasons $reasons `
            -Code 'source.dirty' -Scope 'readiness' `
            -Message 'Public candidates require a clean source tree.'
    }
    if ($sourceCommit -ne $commit) {
        Add-PublicCandidateReason -Reasons $reasons `
            -Code 'source.commit.stale' -Scope 'readiness' `
            -Message 'The source observation is stale for this candidate commit.'
    }

    $expected = Get-PublicCandidateProperty -Object $Candidate -Name 'expected'
    $expectedGateHash = Get-PublicCandidateProperty `
        -Object $expected -Name 'gate_manifest_sha256'
    $expectedArtifactManifestHash = Get-PublicCandidateProperty `
        -Object $expected -Name 'artifact_manifest_sha256'
    $expectedCargoHash = Get-PublicCandidateProperty `
        -Object $expected -Name 'cargo_lock_sha256'
    $expectedSbomHash = Get-PublicCandidateProperty `
        -Object $expected -Name 'sbom_sha256'
    foreach ($hashSpec in @(
            @{ value = $expectedGateHash; label = 'gate manifest' }
            @{
                value = $expectedArtifactManifestHash
                label = 'artifact manifest'
            }
            @{ value = $expectedCargoHash; label = 'Cargo.lock' }
            @{ value = $expectedSbomHash; label = 'SBOM' }
        )) {
        if (-not (Test-PublicCandidateHash -Value $hashSpec.value)) {
            Add-PublicCandidateReason -Reasons $reasons `
                -Code 'expected.hash.invalid' -Scope 'readiness' `
                -Message "The expected $($hashSpec.label) hash is missing or invalid."
        }
    }

    $qualification = Get-PublicCandidateProperty `
        -Object $Candidate -Name 'qualification'
    $qualificationStatus = [string](
        Get-PublicCandidateProperty -Object $qualification -Name 'status'
    )
    $qualificationUsable = $false
    if ($null -eq $qualification -or
        [string]::IsNullOrWhiteSpace($qualificationStatus) -or
        $qualificationStatus -eq 'missing') {
        Add-PublicCandidateReason -Reasons $reasons `
            -Code 'qualification.receipt.missing' -Scope 'readiness' `
            -Message 'The stress-inclusive qualification receipt is missing.'
    } elseif ($qualificationStatus -eq 'skipped') {
        Add-PublicCandidateReason -Reasons $reasons `
            -Code 'qualification.receipt.skipped' -Scope 'readiness' `
            -Message 'Qualification was explicitly skipped.'
    } elseif ($qualificationStatus -ne 'complete') {
        Add-PublicCandidateReason -Reasons $reasons `
            -Code 'qualification.receipt.incomplete' -Scope 'readiness' `
            -Message 'Qualification did not complete successfully.'
    } else {
        $qualificationUsable = $true
        if ((Get-PublicCandidateProperty -Object $qualification -Name 'release') -isnot [bool] -or
            -not [bool](Get-PublicCandidateProperty -Object $qualification -Name 'release')) {
            Add-PublicCandidateReason -Reasons $reasons `
                -Code 'qualification.release-mode.required' -Scope 'readiness' `
                -Message 'The receipt was not produced by release qualification.'
        }
        if ((Get-PublicCandidateProperty -Object $qualification -Name 'stress_included') -isnot [bool] -or
            -not [bool](Get-PublicCandidateProperty -Object $qualification -Name 'stress_included')) {
            Add-PublicCandidateReason -Reasons $reasons `
                -Code 'qualification.stress.required' -Scope 'readiness' `
                -Message 'The receipt does not include the required stress gate.'
        }
        if ((Get-PublicCandidateProperty -Object $qualification -Name 'all_gates_passed') -isnot [bool] -or
            -not [bool](Get-PublicCandidateProperty -Object $qualification -Name 'all_gates_passed')) {
            Add-PublicCandidateReason -Reasons $reasons `
                -Code 'qualification.gates.incomplete' -Scope 'readiness' `
                -Message 'Not every required qualification gate passed.'
        }
        if ([string](Get-PublicCandidateProperty `
                -Object $qualification -Name 'commit') -ne $commit) {
            Add-PublicCandidateReason -Reasons $reasons `
                -Code 'qualification.commit.stale' -Scope 'readiness' `
                -Message 'The qualification receipt belongs to another commit.'
        }
        $receiptHash = Get-PublicCandidateProperty `
            -Object $qualification -Name 'receipt_sha256'
        if (-not (Test-PublicCandidateHash -Value $receiptHash)) {
            Add-PublicCandidateReason -Reasons $reasons `
                -Code 'qualification.receipt-hash.invalid' -Scope 'readiness' `
                -Message 'The qualification receipt hash is missing or invalid.'
        }
        Test-PublicCandidateExactHash `
            -ExpectedValue $expectedGateHash `
            -ActualValue (Get-PublicCandidateProperty `
                -Object $qualification -Name 'gate_manifest_sha256') `
            -MissingCode 'qualification.gate-manifest-hash.missing' `
            -InvalidCode 'qualification.gate-manifest-hash.invalid' `
            -MismatchCode 'qualification.gate-manifest-hash.mismatch' `
            -Label 'Qualification gate manifest hash' `
            -Reasons $reasons | Out-Null
        Test-PublicCandidateExactHash `
            -ExpectedValue $expectedArtifactManifestHash `
            -ActualValue (Get-PublicCandidateProperty `
                -Object $qualification -Name 'artifact_manifest_sha256') `
            -MissingCode 'qualification.artifact-manifest-hash.missing' `
            -InvalidCode 'qualification.artifact-manifest-hash.invalid' `
            -MismatchCode 'qualification.artifact-manifest-hash.mismatch' `
            -Label 'Qualification artifact manifest hash' `
            -Reasons $reasons | Out-Null
        Test-PublicCandidateExactHash `
            -ExpectedValue $expectedCargoHash `
            -ActualValue (Get-PublicCandidateProperty `
                -Object $qualification -Name 'cargo_lock_sha256') `
            -MissingCode 'qualification.cargo-lock-hash.missing' `
            -InvalidCode 'qualification.cargo-lock-hash.invalid' `
            -MismatchCode 'qualification.cargo-lock-hash.mismatch' `
            -Label 'Qualification Cargo.lock hash' `
            -Reasons $reasons | Out-Null
        Test-PublicCandidateExactHash `
            -ExpectedValue $expectedSbomHash `
            -ActualValue (Get-PublicCandidateProperty `
                -Object $qualification -Name 'sbom_sha256') `
            -MissingCode 'qualification.sbom-hash.missing' `
            -InvalidCode 'qualification.sbom-hash.invalid' `
            -MismatchCode 'qualification.sbom-hash.mismatch' `
            -Label 'Qualification SBOM hash' `
            -Reasons $reasons | Out-Null
        Compare-PublicCandidateArtifacts `
            -Expected (Get-PublicCandidateProperty `
                -Object $expected -Name 'artifacts') `
            -Actual (Get-PublicCandidateProperty `
                -Object $qualification -Name 'artifacts') `
            -Prefix 'qualification' -Reasons $reasons | Out-Null
    }

    $package = Get-PublicCandidateProperty -Object $Candidate -Name 'package'
    $packageStatus = [string](
        Get-PublicCandidateProperty -Object $package -Name 'status'
    )
    if ($null -eq $package -or [string]::IsNullOrWhiteSpace($packageStatus) -or
        $packageStatus -eq 'missing') {
        Add-PublicCandidateReason -Reasons $reasons `
            -Code 'package.provenance.missing' -Scope 'readiness' `
            -Message 'The same-byte package provenance is missing.'
    } elseif ($packageStatus -eq 'skipped') {
        Add-PublicCandidateReason -Reasons $reasons `
            -Code 'package.provenance.skipped' -Scope 'readiness' `
            -Message 'Candidate packaging was explicitly skipped.'
    } elseif ($packageStatus -ne 'complete') {
        Add-PublicCandidateReason -Reasons $reasons `
            -Code 'package.provenance.incomplete' -Scope 'readiness' `
            -Message 'Candidate packaging did not complete successfully.'
    } else {
        if ([string](Get-PublicCandidateProperty `
                -Object $package -Name 'qualified_commit') -ne $commit) {
            Add-PublicCandidateReason -Reasons $reasons `
                -Code 'package.commit.stale' -Scope 'readiness' `
                -Message 'The package provenance belongs to another commit.'
        }
        if ((Get-PublicCandidateProperty -Object $package -Name 'no_rebuild') -isnot [bool] -or
            -not [bool](Get-PublicCandidateProperty -Object $package -Name 'no_rebuild') -or
            [int](Get-PublicCandidateProperty `
                -Object $package -Name 'build_invocations') -ne 0 -or
            [string](Get-PublicCandidateProperty `
                -Object $package -Name 'source_kind') -ne 'qualified_bytes') {
            Add-PublicCandidateReason -Reasons $reasons `
                -Code 'package.rebuild.detected' -Scope 'readiness' `
                -Message 'Packaging was not proven to consume qualified bytes without rebuilding.'
        }
        if ($qualificationUsable) {
            Test-PublicCandidateExactHash `
                -ExpectedValue (Get-PublicCandidateProperty `
                    -Object $qualification -Name 'receipt_sha256') `
                -ActualValue (Get-PublicCandidateProperty `
                    -Object $package -Name 'qualification_receipt_sha256') `
                -MissingCode 'package.receipt-hash.missing' `
                -InvalidCode 'package.receipt-hash.invalid' `
                -MismatchCode 'package.receipt-hash.mismatch' `
                -Label 'Packaged qualification receipt hash' `
                -Reasons $reasons | Out-Null
            Test-PublicCandidateExactHash `
                -ExpectedValue (Get-PublicCandidateProperty `
                    -Object $qualification -Name 'artifact_manifest_sha256') `
                -ActualValue (Get-PublicCandidateProperty `
                    -Object $package -Name 'artifact_manifest_sha256') `
                -MissingCode 'package.artifact-manifest-hash.missing' `
                -InvalidCode 'package.artifact-manifest-hash.invalid' `
                -MismatchCode 'package.artifact-manifest-hash.mismatch' `
                -Label 'Packaged artifact manifest hash' `
                -Reasons $reasons | Out-Null
            Test-PublicCandidateExactHash `
                -ExpectedValue (Get-PublicCandidateProperty `
                    -Object $qualification -Name 'sbom_sha256') `
                -ActualValue (Get-PublicCandidateProperty `
                    -Object $package -Name 'sbom_sha256') `
                -MissingCode 'package.sbom-hash.missing' `
                -InvalidCode 'package.sbom-hash.invalid' `
                -MismatchCode 'package.sbom-hash.mismatch' `
                -Label 'Packaged SBOM hash' `
                -Reasons $reasons | Out-Null
            Compare-PublicCandidateArtifacts `
                -Expected (Get-PublicCandidateProperty `
                    -Object $qualification -Name 'artifacts') `
                -Actual (Get-PublicCandidateProperty `
                    -Object $package -Name 'artifacts') `
                -Prefix 'package' -Reasons $reasons | Out-Null
        }
        $archiveHash = Get-PublicCandidateProperty `
            -Object $package -Name 'archive_sha256'
        $observedArchiveHash = Get-PublicCandidateProperty `
            -Object $package -Name 'observed_archive_sha256'
        if (-not (Test-PublicCandidateHash -Value $archiveHash) -or
            -not (Test-PublicCandidateHash -Value $observedArchiveHash)) {
            Add-PublicCandidateReason -Reasons $reasons `
                -Code 'package.archive-hash.invalid' -Scope 'readiness' `
                -Message 'The package archive hash evidence is missing or invalid.'
        } elseif ([string]$archiveHash -cne [string]$observedArchiveHash) {
            Add-PublicCandidateReason -Reasons $reasons `
                -Code 'package.archive.tampered' -Scope 'readiness' `
                -Message 'The observed archive bytes differ from package provenance.'
        }
    }

    $baseBlockers = @($reasons | Where-Object {
        $_.scope -eq 'readiness'
    })
    $baseReady = $baseBlockers.Count -eq 0

    $rehearsal = Get-PublicCandidateProperty `
        -Object $Candidate -Name 'rehearsal'
    $rehearsalStatus = [string](
        Get-PublicCandidateProperty -Object $rehearsal -Name 'status'
    )
    if ($null -eq $rehearsal -or
        [string]::IsNullOrWhiteSpace($rehearsalStatus) -or
        $rehearsalStatus -eq 'missing') {
        Add-PublicCandidateReason -Reasons $reasons `
            -Code 'rehearsal.missing' -Scope 'readiness' `
            -Message 'The publication rehearsal result is missing.'
    } elseif ($rehearsalStatus -eq 'skipped') {
        Add-PublicCandidateReason -Reasons $reasons `
            -Code 'rehearsal.skipped' -Scope 'readiness' `
            -Message 'The publication rehearsal was skipped.'
    } elseif ($rehearsalStatus -ne 'passed') {
        Add-PublicCandidateReason -Reasons $reasons `
            -Code 'rehearsal.failed' -Scope 'readiness' `
            -Message 'The publication rehearsal did not pass.'
    } else {
        if ([string](Get-PublicCandidateProperty `
                -Object $rehearsal -Name 'commit') -ne $commit) {
            Add-PublicCandidateReason -Reasons $reasons `
                -Code 'rehearsal.commit.stale' -Scope 'readiness' `
                -Message 'The rehearsal belongs to another commit.'
        }
        if ([string](Get-PublicCandidateProperty `
                -Object $rehearsal -Name 'tag') -ne $tag) {
            Add-PublicCandidateReason -Reasons $reasons `
                -Code 'rehearsal.tag.mismatch' -Scope 'readiness' `
                -Message 'The rehearsal tag does not match the candidate tag.'
        }
        if ([string](Get-PublicCandidateProperty `
                -Object $rehearsal -Name 'archive_sha256') -cne
            [string](Get-PublicCandidateProperty `
                -Object $package -Name 'archive_sha256')) {
            Add-PublicCandidateReason -Reasons $reasons `
                -Code 'rehearsal.archive-hash.mismatch' -Scope 'readiness' `
                -Message 'The rehearsal did not use the exact candidate archive.'
        }
    }

    $readinessBlockers = @($reasons | Where-Object {
        $_.scope -eq 'readiness'
    })
    $publicReady = $readinessBlockers.Count -eq 0
    $internalOnly = $version -eq '0.1.7'
    $rehearsalAllowed = $baseReady -and -not $internalOnly

    $approval = Get-PublicCandidateProperty -Object $Candidate -Name 'approval'
    $approvalStatus = [string](
        Get-PublicCandidateProperty -Object $approval -Name 'status'
    )
    if ($null -eq $approval -or [string]::IsNullOrWhiteSpace($approvalStatus) -or
        $approvalStatus -eq 'missing') {
        Add-PublicCandidateReason -Reasons $reasons `
            -Code 'approval.missing' -Scope 'authorization' `
            -Message 'Publication has not received explicit approval.'
    } elseif ($approvalStatus -ne 'approved') {
        Add-PublicCandidateReason -Reasons $reasons `
            -Code 'approval.denied' -Scope 'authorization' `
            -Message 'Publication approval is not approved.'
    } else {
        if ([string](Get-PublicCandidateProperty `
                -Object $approval -Name 'version') -ne $version) {
            Add-PublicCandidateReason -Reasons $reasons `
                -Code 'approval.version.stale' -Scope 'authorization' `
                -Message 'Publication approval belongs to another version.'
        }
        if ([string](Get-PublicCandidateProperty `
                -Object $approval -Name 'tag') -ne $tag) {
            Add-PublicCandidateReason -Reasons $reasons `
                -Code 'approval.tag.stale' -Scope 'authorization' `
                -Message 'Publication approval belongs to another tag.'
        }
        if ([string](Get-PublicCandidateProperty `
                -Object $approval -Name 'commit') -ne $commit) {
            Add-PublicCandidateReason -Reasons $reasons `
                -Code 'approval.commit.stale' -Scope 'authorization' `
                -Message 'Publication approval belongs to another commit.'
        }
        if ([string](Get-PublicCandidateProperty `
                -Object $approval -Name 'archive_sha256') -cne
            [string](Get-PublicCandidateProperty `
                -Object $package -Name 'archive_sha256')) {
            Add-PublicCandidateReason -Reasons $reasons `
                -Code 'approval.archive-hash.stale' -Scope 'authorization' `
                -Message 'Publication approval belongs to another archive.'
        }
    }
    $authorizationBlockers = @($reasons | Where-Object {
        $_.scope -eq 'authorization'
    })
    $publishAuthorized = (
        $publicReady -and
        -not $internalOnly -and
        $authorizationBlockers.Count -eq 0
    )
    $decision = if ($publishAuthorized) {
        'publish_authorized'
    } elseif ($publicReady) {
        'public_ready'
    } elseif ($rehearsalAllowed) {
        'rehearsal_only'
    } else {
        'rejected'
    }

    return [ordered]@{
        schema_version = $script:PublicCandidatePolicySchemaVersion
        kind = $script:PublicCandidatePolicyKind
        version = $version
        tag = $tag
        commit = $commit
        decision = $decision
        rehearsal_allowed = $rehearsalAllowed
        public_ready = $publicReady
        publish_authorized = $publishAuthorized
        reason_codes = @($reasons | ForEach-Object { $_.code })
        reasons = @($reasons)
    }
}
