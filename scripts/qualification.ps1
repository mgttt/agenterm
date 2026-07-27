$script:QualificationEvidencePattern = '^EVIDENCE (?<id>[a-z0-9][a-z0-9.-]+)$'

function Get-QualificationSha256 {
    param([Parameter(Mandatory = $true)][string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "Qualification input does not exist: $Path"
    }
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).
        Hash.ToLowerInvariant()
}

function Read-AgenTermQualificationManifest {
    param([Parameter(Mandatory = $true)][string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "Qualification gate manifest does not exist: $Path"
    }
    $manifest = Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
    if ($manifest.schema_version -ne 1 -or
        $manifest.receipt_schema_version -ne 1) {
        throw 'Unsupported qualification gate manifest schema.'
    }
    $gates = @($manifest.required_gates)
    if ($gates.Count -eq 0) {
        throw 'Qualification gate manifest contains no required gates.'
    }
    $gateIds = [Collections.Generic.HashSet[string]]::new(
        [StringComparer]::Ordinal
    )
    $evidenceIds = [Collections.Generic.HashSet[string]]::new(
        [StringComparer]::Ordinal
    )
    foreach ($gate in $gates) {
        $gateId = [string]$gate.id
        if ($gateId -notmatch '^[a-z0-9][a-z0-9-]+$' -or
            -not $gateIds.Add($gateId)) {
            throw "Invalid or duplicate qualification gate ID: $gateId"
        }
        foreach ($evidenceId in @($gate.evidence)) {
            if ([string]$evidenceId -notmatch '^[a-z0-9][a-z0-9.-]+$' -or
                -not $evidenceIds.Add([string]$evidenceId)) {
                throw "Invalid or duplicate qualification evidence ID: $evidenceId"
            }
        }
    }
    return $manifest
}

function New-AgenTermQualificationContext {
    param(
        [Parameter(Mandatory = $true)][string]$ManifestPath,
        [Parameter(Mandatory = $true)][bool]$Release,
        [Parameter(Mandatory = $true)][bool]$StressIncluded
    )

    $resolvedManifestPath = (Resolve-Path -LiteralPath $ManifestPath).Path
    $manifest = Read-AgenTermQualificationManifest -Path $resolvedManifestPath
    return [pscustomobject]@{
        Manifest = $manifest
        ManifestPath = $resolvedManifestPath
        ManifestSha256 = Get-QualificationSha256 -Path $resolvedManifestPath
        Release = $Release
        StressIncluded = $StressIncluded
        StartedAtUtc = [DateTime]::UtcNow.ToString('o')
        Results = [ordered]@{}
    }
}

function Assert-AgenTermQualificationDeclarations {
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)][hashtable]$SuiteScripts
    )

    foreach ($gateId in $SuiteScripts.Keys) {
        $gate = @($Context.Manifest.required_gates) |
            Where-Object { $_.id -eq $gateId }
        if ($gate.Count -ne 1) {
            throw "Evidence suite references undeclared qualification gate: $gateId"
        }
        $actual = @(& $SuiteScripts[$gateId] -ListEvidence 2>&1)
        if ($LASTEXITCODE -ne 0) {
            throw "$gateId -ListEvidence failed: $($actual -join "`n")"
        }
        $expected = @($gate[0].evidence | ForEach-Object { [string]$_ } |
            Sort-Object)
        $actual = @($actual | ForEach-Object { [string]$_ } | Sort-Object)
        if (($expected -join "`n") -ne ($actual -join "`n")) {
            throw "Qualification manifest declarations are stale: $gateId"
        }
    }
}

function Add-AgenTermQualificationResult {
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)][string]$GateId,
        [Parameter(Mandatory = $true)]
        [ValidateSet('passed', 'failed', 'skipped')]
        [string]$Status,
        [Parameter(Mandatory = $true)][long]$DurationMs,
        [AllowNull()][object[]]$Output
    )

    if ($Context.Results.Contains($GateId)) {
        throw "Qualification gate was recorded more than once: $GateId"
    }
    $declaredGate = @($Context.Manifest.required_gates) |
        Where-Object { $_.id -eq $GateId }
    if ($declaredGate.Count -ne 1) {
        throw "Qualification result references undeclared gate: $GateId"
    }
    $emitted = [Collections.Generic.List[string]]::new()
    foreach ($item in @($Output)) {
        $line = [string]$item
        $match = [regex]::Match($line.Trim(), $script:QualificationEvidencePattern)
        if ($match.Success) {
            $emitted.Add($match.Groups['id'].Value)
        }
    }
    $Context.Results[$GateId] = [pscustomobject]@{
        id = $GateId
        status = $Status
        duration_ms = $DurationMs
        evidence = @($emitted)
    }
}

function Assert-AgenTermQualificationResults {
    param([Parameter(Mandatory = $true)]$Context)

    foreach ($gate in @($Context.Manifest.required_gates)) {
        $gateId = [string]$gate.id
        if (-not $Context.Results.Contains($gateId)) {
            throw "Required qualification gate did not run: $gateId"
        }
        $result = $Context.Results[$gateId]
        if ($result.status -ne 'passed') {
            throw "Required qualification gate is not passed: $gateId ($($result.status))"
        }
        $expected = @($gate.evidence | ForEach-Object { [string]$_ } | Sort-Object)
        $actual = @($result.evidence | ForEach-Object { [string]$_ } | Sort-Object)
        if (($actual | Select-Object -Unique).Count -ne $actual.Count) {
            throw "Qualification gate emitted duplicate evidence: $gateId"
        }
        if (($expected -join "`n") -ne ($actual -join "`n")) {
            throw "Qualification evidence does not match the manifest: $gateId"
        }
    }
    if (-not $Context.StressIncluded) {
        throw 'Internal qualification receipt requires the explicit stress gate.'
    }
}

function Get-AgenTermQualificationProvenance {
    param(
        [Parameter(Mandatory = $true)][string]$RepoRoot,
        [Parameter(Mandatory = $true)][string]$BuildMetadataPath,
        [Parameter(Mandatory = $true)][string]$ArtifactManifestPath,
        [Parameter(Mandatory = $true)][string]$StagedDirectory,
        [Parameter(Mandatory = $true)][bool]$RequireClean
    )

    $headLines = @(& git -C $RepoRoot rev-parse HEAD 2>&1)
    if ($LASTEXITCODE -ne 0) {
        throw "Could not resolve qualification Git HEAD: $($headLines -join "`n")"
    }
    $head = ([string]$headLines[0]).Trim().ToLowerInvariant()
    if ($head -notmatch '^[0-9a-f]{40}$') {
        throw "Qualification Git HEAD is not a full commit ID: $head"
    }
    $metadata = Get-Content -LiteralPath $BuildMetadataPath -Raw |
        ConvertFrom-Json
    if ($metadata.schema_version -ne 2) {
        throw 'Qualification requires build metadata schema 2.'
    }
    if ([string]$metadata.git_commit -ne $head) {
        throw 'Build metadata Git commit does not match exact qualification HEAD.'
    }
    if ($RequireClean -and [bool]$metadata.git_dirty) {
        throw 'Release qualification refuses build metadata from a dirty source tree.'
    }
    $cargoLockHash = Get-QualificationSha256 -Path (
        Join-Path $RepoRoot 'Cargo.lock'
    )
    if ([string]$metadata.cargo_lock_sha256 -ne $cargoLockHash) {
        throw 'Build metadata Cargo.lock hash mismatch.'
    }
    $artifactManifestHash = Get-QualificationSha256 -Path $ArtifactManifestPath
    if ([string]$metadata.artifact_manifest_sha256 -ne $artifactManifestHash) {
        throw 'Build metadata artifact manifest hash mismatch.'
    }
    $artifactSpec = Get-Content -LiteralPath $ArtifactManifestPath -Raw |
        ConvertFrom-Json
    $expectedNames = @($artifactSpec.executables.name)
    if ($expectedNames.Count -ne 4) {
        throw 'Qualification requires exactly four staged executables.'
    }
    $artifactRecords = @(
        foreach ($name in $expectedNames) {
            $path = Join-Path $StagedDirectory $name
            $hash = Get-QualificationSha256 -Path $path
            $metadataEntry = @($metadata.executables) |
                Where-Object { $_.name -eq $name }
            if ($metadataEntry.Count -ne 1 -or
                [string]$metadataEntry[0].sha256 -ne $hash) {
                throw "Build metadata staged executable hash mismatch: $name"
            }
            [ordered]@{
                name = [string]$name
                sha256 = $hash
            }
        }
    )
    $sbomHash = Get-QualificationSha256 -Path (
        Join-Path $StagedDirectory 'agenterm-sbom.spdx.json'
    )
    return [ordered]@{
        git_head = $head
        source_dirty = [bool]$metadata.git_dirty
        cargo_lock_sha256 = $cargoLockHash
        artifact_manifest_sha256 = $artifactManifestHash
        sbom_sha256 = $sbomHash
        executables = $artifactRecords
    }
}

function Write-AgenTermQualificationReceipt {
    param(
        [Parameter(Mandatory = $true)]$Context,
        [Parameter(Mandatory = $true)][string]$RepoRoot,
        [Parameter(Mandatory = $true)][string]$OutputPath
    )

    Assert-AgenTermQualificationResults -Context $Context
    $provenance = Get-AgenTermQualificationProvenance `
        -RepoRoot $RepoRoot `
        -BuildMetadataPath (Join-Path $RepoRoot 'dist\agenterm.json') `
        -ArtifactManifestPath (Join-Path $RepoRoot 'scripts\artifacts.json') `
        -StagedDirectory (Join-Path $RepoRoot 'dist') `
        -RequireClean $true
    $receipt = [ordered]@{
        schema_version = [int]$Context.Manifest.receipt_schema_version
        product = 'AgenTerm'
        completed_at_utc = [DateTime]::UtcNow.ToString('o')
        started_at_utc = $Context.StartedAtUtc
        release = $Context.Release
        stress_included = $Context.StressIncluded
        gate_manifest = [ordered]@{
            path = 'scripts/qualification-gates.json'
            sha256 = $Context.ManifestSha256
        }
        provenance = $provenance
        gates = @(
            foreach ($gate in @($Context.Manifest.required_gates)) {
                $Context.Results[[string]$gate.id]
            }
        )
    }
    $parent = Split-Path -Parent ([IO.Path]::GetFullPath($OutputPath))
    New-Item -ItemType Directory -Path $parent -Force | Out-Null
    $temporary = Join-Path $parent (
        ".$([IO.Path]::GetFileName($OutputPath)).$PID.tmp"
    )
    try {
        $json = $receipt | ConvertTo-Json -Depth 8
        [IO.File]::WriteAllText(
            $temporary,
            "$json`n",
            [Text.UTF8Encoding]::new($false)
        )
        Move-Item -LiteralPath $temporary -Destination $OutputPath -Force
    }
    finally {
        if (Test-Path -LiteralPath $temporary) {
            Remove-Item -LiteralPath $temporary -Force
        }
    }
    return [IO.Path]::GetFullPath($OutputPath)
}
