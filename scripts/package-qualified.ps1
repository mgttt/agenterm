param(
    [string]$ReceiptPath = 'target\qualification\receipt.json',
    [string]$OutputDirectory = 'target\qualification\package-dry-run',
    [string]$RepoRoot = (Join-Path $PSScriptRoot '..')
)

$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath($RepoRoot)

function Resolve-PackagePath {
    param(
        [Parameter(Mandatory = $true)][string]$Base,
        [Parameter(Mandatory = $true)][string]$Path
    )
    if ([IO.Path]::IsPathRooted($Path)) {
        return [IO.Path]::GetFullPath($Path)
    }
    return [IO.Path]::GetFullPath((Join-Path $Base $Path))
}

$receiptFile = Resolve-PackagePath -Base $repo -Path $ReceiptPath
$outputRoot = Resolve-PackagePath -Base $repo -Path $OutputDirectory

function Get-PackageSha256 {
    param([Parameter(Mandatory = $true)][string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "Package qualification input does not exist: $Path"
    }
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).
        Hash.ToLowerInvariant()
}

function Compare-PackageNames {
    param(
        [Parameter(Mandatory = $true)][string[]]$Expected,
        [Parameter(Mandatory = $true)][string[]]$Actual,
        [Parameter(Mandatory = $true)][string]$Label
    )

    $expectedSorted = @($Expected | Sort-Object)
    $actualSorted = @($Actual | Sort-Object)
    if (($expectedSorted -join "`n") -ne ($actualSorted -join "`n")) {
        throw "$Label does not contain the exact required set."
    }
}

if (-not (Test-Path -LiteralPath $receiptFile -PathType Leaf)) {
    throw "Qualification receipt does not exist: $receiptFile"
}
$receipt = Get-Content -LiteralPath $receiptFile -Raw | ConvertFrom-Json
if ($receipt.schema_version -ne 1 -or
    $receipt.product -ne 'AgenTerm' -or
    -not [bool]$receipt.release -or
    -not [bool]$receipt.stress_included) {
    throw 'Packaging requires a release qualification receipt with stress included.'
}
if ([bool]$receipt.provenance.source_dirty) {
    throw 'Packaging refuses a qualification receipt from a dirty source tree.'
}
foreach ($gate in @($receipt.gates)) {
    if ($gate.status -ne 'passed') {
        throw "Qualification receipt contains a non-passing gate: $($gate.id)"
    }
}

$headOutput = @(& git -C $repo rev-parse HEAD 2>&1)
if ($LASTEXITCODE -ne 0) {
    throw "Could not resolve package source HEAD: $($headOutput -join "`n")"
}
$head = ([string]$headOutput[0]).Trim().ToLowerInvariant()
if ($head -notmatch '^[0-9a-f]{40}$' -or
    [string]$receipt.provenance.git_head -ne $head) {
    throw 'Qualification receipt HEAD does not match the exact package source HEAD.'
}

$cargoLockHash = Get-PackageSha256 -Path (Join-Path $repo 'Cargo.lock')
if ([string]$receipt.provenance.cargo_lock_sha256 -ne $cargoLockHash) {
    throw 'Qualification receipt Cargo.lock hash mismatch.'
}
$artifactManifestPath = Join-Path $repo 'scripts\artifacts.json'
$artifactManifestHash = Get-PackageSha256 -Path $artifactManifestPath
if ([string]$receipt.provenance.artifact_manifest_sha256 -ne
    $artifactManifestHash) {
    throw 'Qualification receipt artifact manifest hash mismatch.'
}
$sbomPath = Join-Path $repo 'dist\agenterm-sbom.spdx.json'
$sbomHash = Get-PackageSha256 -Path $sbomPath
if ([string]$receipt.provenance.sbom_sha256 -ne $sbomHash) {
    throw 'Qualification receipt SPDX inventory hash mismatch.'
}
$gateManifestPath = Join-Path $repo 'scripts\qualification-gates.json'
$gateManifestHash = Get-PackageSha256 -Path $gateManifestPath
if ([string]$receipt.gate_manifest.sha256 -ne $gateManifestHash) {
    throw 'Qualification receipt gate manifest hash mismatch.'
}
$gateManifest = Get-Content -LiteralPath $gateManifestPath -Raw |
    ConvertFrom-Json
$requiredGateIds = @($gateManifest.required_gates |
    ForEach-Object { [string]$_.id })
$receiptGateIds = @($receipt.gates |
    ForEach-Object { [string]$_.id })
Compare-PackageNames -Expected $requiredGateIds -Actual $receiptGateIds `
    -Label 'Qualification receipt gates'

$artifactSpec = Get-Content -LiteralPath $artifactManifestPath -Raw |
    ConvertFrom-Json
$artifactNames = @($artifactSpec.executables | ForEach-Object {
    [string]$_.name
})
if ($artifactNames.Count -ne 4) {
    throw 'Package qualification requires exactly four executable artifacts.'
}
$receiptExecutables = @($receipt.provenance.executables)
Compare-PackageNames -Expected $artifactNames `
    -Actual @($receiptExecutables | ForEach-Object { [string]$_.name }) `
    -Label 'Qualification receipt'

$metadataPath = Join-Path $repo 'dist\agenterm.json'
$metadata = Get-Content -LiteralPath $metadataPath -Raw | ConvertFrom-Json
if ($metadata.schema_version -ne 2 -or
    [string]$metadata.git_commit -ne $head -or
    [bool]$metadata.git_dirty -or
    [string]$metadata.cargo_lock_sha256 -ne $cargoLockHash -or
    [string]$metadata.artifact_manifest_sha256 -ne $artifactManifestHash) {
    throw 'Build metadata does not match the qualified source and locked inputs.'
}
Compare-PackageNames -Expected $artifactNames `
    -Actual @($metadata.executables | ForEach-Object { [string]$_.name }) `
    -Label 'Build metadata'

$payload = [Collections.Generic.List[object]]::new()
foreach ($name in $artifactNames) {
    $path = Join-Path (Join-Path $repo 'dist') $name
    $hash = Get-PackageSha256 -Path $path
    $receiptEntry = @($receiptExecutables |
        Where-Object { $_.name -eq $name })
    $metadataEntry = @($metadata.executables |
        Where-Object { $_.name -eq $name })
    if ($receiptEntry.Count -ne 1 -or $metadataEntry.Count -ne 1 -or
        [string]$receiptEntry[0].sha256 -ne $hash -or
        [string]$metadataEntry[0].sha256 -ne $hash) {
        throw "Qualified executable SHA-256 mismatch: $name"
    }
    $payload.Add([ordered]@{
        name = $name
        sha256 = $hash
    })
}

$staticPayload = @(
    @{ name = 'agenterm.json'; path = $metadataPath }
    @{ name = 'artifacts.json'; path = $artifactManifestPath }
    @{ name = 'qualification-receipt.json'; path = $receiptFile }
    @{ name = 'agenterm-sbom.spdx.json'; path = $sbomPath }
    @{ name = 'LICENSE-APACHE'; path = (Join-Path $repo 'LICENSE-APACHE') }
    @{ name = 'LICENSE-MIT'; path = (Join-Path $repo 'LICENSE-MIT') }
    @{
        name = 'THIRD_PARTY_NOTICES.md'
        path = (Join-Path $repo 'THIRD_PARTY_NOTICES.md')
    }
)
foreach ($entry in $staticPayload) {
    $payload.Add([ordered]@{
        name = [string]$entry.name
        sha256 = Get-PackageSha256 -Path ([string]$entry.path)
    })
}

$version = [string]$metadata.version
if ($version -notmatch '^\d+\.\d+\.\d+([+-][0-9A-Za-z.-]+)?$') {
    throw "Build metadata contains an invalid package version: $version"
}
$runId = '{0}-{1}' -f
    [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssfffZ'),
    ([Guid]::NewGuid().ToString('N').Substring(0, 8))
$runDirectory = Join-Path $outputRoot $runId
$staging = Join-Path $runDirectory 'staging'
[IO.Directory]::CreateDirectory($staging) | Out-Null

try {
    foreach ($entry in @($payload)) {
        $source = switch ([string]$entry.name) {
            'agenterm.json' { $metadataPath }
            'artifacts.json' { $artifactManifestPath }
            'qualification-receipt.json' { $receiptFile }
            'agenterm-sbom.spdx.json' { $sbomPath }
            default {
                if ($artifactNames -contains [string]$entry.name) {
                    Join-Path (Join-Path $repo 'dist') ([string]$entry.name)
                } else {
                    Join-Path $repo ([string]$entry.name)
                }
            }
        }
        $stagedPath = Join-Path $staging ([string]$entry.name)
        Copy-Item -LiteralPath $source -Destination $stagedPath
        if ((Get-PackageSha256 -Path $stagedPath) -ne
            [string]$entry.sha256) {
            throw "Qualified payload changed while staging: $($entry.name)"
        }
    }
    $packageManifest = [ordered]@{
        schema_version = 1
        dry_run = $true
        product = 'AgenTerm'
        version = $version
        qualified_head = $head
        qualification_receipt_sha256 = Get-PackageSha256 -Path $receiptFile
        cargo_lock_sha256 = $cargoLockHash
        artifact_manifest_sha256 = $artifactManifestHash
        sbom_sha256 = $sbomHash
        payload = @($payload)
    }
    $packageManifestPath = Join-Path $staging 'package-manifest.json'
    $packageManifest | ConvertTo-Json -Depth 6 |
        Set-Content -LiteralPath $packageManifestPath -Encoding UTF8

    $archiveName = "agenterm-$version-windows-x86_64-dry-run.zip"
    $archivePath = Join-Path $runDirectory $archiveName
    Compress-Archive -LiteralPath @(
        Get-ChildItem -LiteralPath $staging -File |
            Select-Object -ExpandProperty FullName
    ) -DestinationPath $archivePath
    $externalManifestPath = Join-Path $runDirectory 'package-manifest.json'
    Copy-Item -LiteralPath $packageManifestPath `
        -Destination $externalManifestPath
    $result = [ordered]@{
        schema_version = 1
        dry_run = $true
        archive = $archiveName
        archive_sha256 = Get-PackageSha256 -Path $archivePath
        package_manifest = 'package-manifest.json'
        qualified_head = $head
    }
    $resultPath = Join-Path $runDirectory 'dry-run-result.json'
    $result | ConvertTo-Json -Depth 4 |
        Set-Content -LiteralPath $resultPath -Encoding UTF8
}
finally {
    if (Test-Path -LiteralPath $staging) {
        [IO.Directory]::Delete($staging, $true)
    }
}

Write-Host "PACKAGE DRY-RUN $runDirectory"
