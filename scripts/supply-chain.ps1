param(
    [string]$RepoRoot = (Join-Path $PSScriptRoot '..'),
    [string]$OutputPath = 'dist\agenterm-sbom.spdx.json'
)

$ErrorActionPreference = 'Stop'
$repoRootPath = [IO.Path]::GetFullPath($RepoRoot)
$outputPathResolved = [IO.Path]::GetFullPath(
    (Join-Path $repoRootPath $OutputPath)
)

function Get-LowerSha256 {
    param([Parameter(Mandatory = $true)][string]$Path)
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-TextSha256 {
    param([Parameter(Mandatory = $true)][string]$Text)
    $bytes = [Text.Encoding]::UTF8.GetBytes($Text)
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace(
            '-', ''
        ).ToLowerInvariant()
    }
    finally {
        $sha.Dispose()
    }
}

function ConvertTo-SpdxLicense {
    param([Parameter(Mandatory = $true)][string]$License)
    switch ($License) {
        'MIT/Apache-2.0' { 'MIT OR Apache-2.0' }
        'MPL-2.0+' { 'MPL-2.0-or-later' }
        default { $License }
    }
}

$lockChecksums = [Collections.Generic.Dictionary[string, string]]::new(
    [StringComparer]::Ordinal
)
$cargoLockPath = Join-Path $repoRootPath 'Cargo.lock'
$cargoLockText = (Get-Content -LiteralPath $cargoLockPath -Raw) -replace
    "\r\n?", "`n"
foreach ($packageBlock in [regex]::Matches(
        $cargoLockText,
        '(?ms)^\[\[package\]\]\r?\n(?<body>.*?)(?=^\[\[package\]\]|\z)'
    )) {
    $body = $packageBlock.Groups['body'].Value
    $name = [regex]::Match(
        $body, '(?m)^name = "(?<value>[^"]+)"$'
    ).Groups['value'].Value
    $version = [regex]::Match(
        $body, '(?m)^version = "(?<value>[^"]+)"$'
    ).Groups['value'].Value
    $source = [regex]::Match(
        $body, '(?m)^source = "(?<value>[^"]+)"$'
    ).Groups['value'].Value
    $checksum = [regex]::Match(
        $body, '(?m)^checksum = "(?<value>[0-9a-f]{64})"$'
    ).Groups['value'].Value
    if (-not [string]::IsNullOrWhiteSpace($checksum)) {
        $lockChecksums["$name`n$version`n$source"] = $checksum
    }
}

$metadataLines = @(
    & cargo metadata --locked --format-version 1 `
        --manifest-path (Join-Path $repoRootPath 'Cargo.toml')
)
if ($LASTEXITCODE -ne 0) {
    throw "cargo metadata --locked failed with exit code $LASTEXITCODE."
}
$metadata = ($metadataLines -join "`n") | ConvertFrom-Json
if ($null -eq $metadata.resolve) {
    throw 'Cargo metadata did not contain a resolved dependency graph.'
}

$allowedLicenses = [Collections.Generic.HashSet[string]]::new(
    [StringComparer]::Ordinal
)
foreach ($license in @(
        '(MIT OR Apache-2.0) AND Unicode-3.0',
        '0BSD OR MIT OR Apache-2.0',
        'Apache-2.0',
        'Apache-2.0 AND MIT',
        'Apache-2.0 OR MIT',
        'Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT',
        'BSD-2-Clause',
        'BSD-2-Clause OR Apache-2.0 OR MIT',
        'BSD-3-Clause',
        'BSD-3-Clause OR MIT OR Apache-2.0',
        'CC0-1.0',
        'CDLA-Permissive-2.0',
        'ISC',
        'MIT',
        'MIT OR Apache-2.0',
        'MIT OR Apache-2.0 OR LGPL-2.1-or-later',
        'MIT OR Apache-2.0 OR Zlib',
        'MIT OR Zlib OR Apache-2.0',
        'MIT/Apache-2.0',
        'MPL-2.0+',
        'Unlicense/MIT',
        'Unlicense OR MIT',
        'Zlib OR Apache-2.0 OR MIT'
    )) {
    [void]$allowedLicenses.Add($license)
}

$workspaceIds = [Collections.Generic.HashSet[string]]::new(
    [StringComparer]::Ordinal
)
foreach ($id in @($metadata.workspace_members)) {
    [void]$workspaceIds.Add([string]$id)
}
$resolvedIds = [Collections.Generic.HashSet[string]]::new(
    [StringComparer]::Ordinal
)
foreach ($node in @($metadata.resolve.nodes)) {
    [void]$resolvedIds.Add([string]$node.id)
}
$dependencies = @(
    $metadata.packages |
        Where-Object {
            $resolvedIds.Contains([string]$_.id) -and
            -not $workspaceIds.Contains([string]$_.id)
        } |
        Sort-Object `
            @{ Expression = { [string]$_.name } },
            @{ Expression = { [string]$_.version } },
            @{ Expression = { [string]$_.source } },
            @{ Expression = { [string]$_.id } }
)
if ($dependencies.Count -eq 0) {
    throw 'Resolved Cargo dependency inventory is empty.'
}

foreach ($package in $dependencies) {
    $license = [string]$package.license
    if ([string]::IsNullOrWhiteSpace($license)) {
        throw "Resolved package $($package.name) $($package.version) has no SPDX license."
    }
    if (-not $allowedLicenses.Contains($license)) {
        throw (
            "Resolved package $($package.name) $($package.version) uses " +
            "unreviewed license expression '$license'."
        )
    }
    if ([string]::IsNullOrWhiteSpace([string]$package.source)) {
        throw "Resolved package $($package.name) $($package.version) has no source."
    }
    $lockKey = "$($package.name)`n$($package.version)`n$($package.source)"
    if ([string]$package.source -like 'registry+*' -and
        -not $lockChecksums.ContainsKey($lockKey)) {
        throw (
            "Registry package $($package.name) $($package.version) has no " +
            'Cargo.lock checksum.'
        )
    }
}

$rootPackage = @($metadata.packages | Where-Object {
        $workspaceIds.Contains([string]$_.id)
    })
if ($rootPackage.Count -ne 1) {
    throw 'Supply-chain inventory requires exactly one workspace package.'
}
$directNames = @(
    $rootPackage[0].dependencies.name |
        Sort-Object -Unique
)
$noticePath = Join-Path $repoRootPath 'THIRD_PARTY_NOTICES.md'
$noticeNames = @(
    Get-Content -LiteralPath $noticePath |
        ForEach-Object {
            $match = [regex]::Match(
                [string]$_,
                '^\| `(?<name>[a-zA-Z0-9_-]+)`(?: \(build dependency\))? \|'
            )
            if ($match.Success) {
                $match.Groups['name'].Value
            }
        } |
        Sort-Object -Unique
)
$noticeDifference = @(Compare-Object $directNames $noticeNames)
if ($noticeDifference.Count -ne 0) {
    throw (
        'THIRD_PARTY_NOTICES.md direct dependency coverage drifted: ' +
        (($noticeDifference | ForEach-Object {
                    "$($_.SideIndicator)$($_.InputObject)"
                }) -join ', ')
    )
}

$cargoLockHash = Get-LowerSha256 -Path $cargoLockPath
$spdxPackages = @(
    foreach ($package in $dependencies) {
        $packageHash = Get-TextSha256 -Text ([string]$package.id)
        $spdxLicense = ConvertTo-SpdxLicense -License ([string]$package.license)
        $record = [ordered]@{
            SPDXID = "SPDXRef-Package-$($packageHash.Substring(0, 16))"
            name = [string]$package.name
            versionInfo = [string]$package.version
            downloadLocation = 'NOASSERTION'
            filesAnalyzed = $false
            licenseConcluded = $spdxLicense
            licenseDeclared = $spdxLicense
            copyrightText = 'NOASSERTION'
            comment = "Cargo source: $($package.source)"
            externalRefs = @(
                [ordered]@{
                    referenceCategory = 'PACKAGE-MANAGER'
                    referenceType = 'purl'
                    referenceLocator = (
                        "pkg:cargo/$($package.name)@$($package.version)"
                    )
                }
            )
        }
        $lockKey = "$($package.name)`n$($package.version)`n$($package.source)"
        if ($lockChecksums.ContainsKey($lockKey)) {
            $record.checksums = @(
                [ordered]@{
                    algorithm = 'SHA256'
                    checksumValue = $lockChecksums[$lockKey]
                }
            )
        }
        [pscustomobject]$record
    }
)
$relationships = @(
    foreach ($package in $spdxPackages) {
        [ordered]@{
            spdxElementId = 'SPDXRef-DOCUMENT'
            relationshipType = 'DESCRIBES'
            relatedSpdxElement = $package.SPDXID
        }
    }
)
$document = [ordered]@{
    spdxVersion = 'SPDX-2.3'
    dataLicense = 'CC0-1.0'
    SPDXID = 'SPDXRef-DOCUMENT'
    name = "AgenTerm Cargo dependencies $cargoLockHash"
    documentNamespace = "https://agenterm.local/spdx/cargo-lock/$cargoLockHash"
    creationInfo = [ordered]@{
        created = '1970-01-01T00:00:00Z'
        creators = @('Tool: scripts/supply-chain.ps1')
        comment = 'Generation time is fixed so identical Cargo.lock inputs produce identical bytes.'
    }
    documentDescribes = @($spdxPackages.SPDXID)
    packages = @($spdxPackages)
    relationships = @($relationships)
}

$parent = Split-Path -Parent $outputPathResolved
New-Item -ItemType Directory -Path $parent -Force | Out-Null
$json = $document | ConvertTo-Json -Depth 10
[IO.File]::WriteAllText(
    $outputPathResolved,
    "$json`n",
    [Text.UTF8Encoding]::new($false)
)
Write-Host (
    "PASS: wrote deterministic SPDX inventory for $($dependencies.Count) " +
    "resolved packages to $outputPathResolved"
)
