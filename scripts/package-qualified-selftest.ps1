$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$ownedRoot = Join-Path $repo 'target\qualification'
$fixture = Join-Path $ownedRoot (
    'package-selftest-' + [Guid]::NewGuid().ToString('N')
)
[IO.Directory]::CreateDirectory($fixture) | Out-Null

function Get-SelfTestSha256 {
    param([Parameter(Mandatory = $true)][string]$Path)
    (Get-FileHash -LiteralPath $Path -Algorithm SHA256).
        Hash.ToLowerInvariant()
}

function Assert-PackageRejected {
    param(
        [Parameter(Mandatory = $true)][scriptblock]$Action,
        [Parameter(Mandatory = $true)][string]$Pattern
    )
    try {
        & $Action
    }
    catch {
        if (($_ | Out-String) -notmatch $Pattern) {
            throw "Expected '$Pattern' rejection, got: $_"
        }
        return
    }
    throw "Expected '$Pattern' rejection, but packaging succeeded."
}

try {
    foreach ($directory in @('dist', 'scripts', 'target\qualification')) {
        [IO.Directory]::CreateDirectory((Join-Path $fixture $directory)) |
            Out-Null
    }
    @(
        '/dist/'
        '/target/'
    ) | Set-Content -LiteralPath (Join-Path $fixture '.gitignore')
    'fixture lock' | Set-Content -LiteralPath (Join-Path $fixture 'Cargo.lock')
    foreach ($name in @(
        'LICENSE-APACHE', 'LICENSE-MIT', 'THIRD_PARTY_NOTICES.md'
    )) {
        "fixture $name" | Set-Content -LiteralPath (Join-Path $fixture $name)
    }
    Copy-Item -LiteralPath (Join-Path $repo 'scripts\artifacts.json') `
        -Destination (Join-Path $fixture 'scripts\artifacts.json')
    Copy-Item -LiteralPath (
        Join-Path $repo 'scripts\qualification-gates.json'
    ) -Destination (
        Join-Path $fixture 'scripts\qualification-gates.json'
    )
    & git -C $fixture init --quiet
    & git -C $fixture add .
    & git -C $fixture -c user.name=qualification-selftest `
        -c user.email=qualification-selftest.invalid commit `
        --quiet -m fixture
    if ($LASTEXITCODE -ne 0) {
        throw 'Could not commit package qualification fixture.'
    }
    $head = (& git -C $fixture rev-parse HEAD).Trim().ToLowerInvariant()
    $artifactPath = Join-Path $fixture 'scripts\artifacts.json'
    $artifactSpec = Get-Content -LiteralPath $artifactPath -Raw |
        ConvertFrom-Json
    $executableRecords = @(
        foreach ($artifact in @($artifactSpec.executables)) {
            $path = Join-Path (Join-Path $fixture 'dist') $artifact.name
            "fixture $($artifact.name)" |
                Set-Content -LiteralPath $path
            [ordered]@{
                name = [string]$artifact.name
                role = [string]$artifact.role
                size = (Get-Item -LiteralPath $path).Length
                sha256 = Get-SelfTestSha256 -Path $path
            }
        }
    )
    $cargoHash = Get-SelfTestSha256 -Path (
        Join-Path $fixture 'Cargo.lock'
    )
    $artifactHash = Get-SelfTestSha256 -Path $artifactPath
    $sbomPath = Join-Path $fixture 'dist\agenterm-sbom.spdx.json'
    'fixture deterministic SPDX inventory' |
        Set-Content -LiteralPath $sbomPath
    $sbomHash = Get-SelfTestSha256 -Path $sbomPath
    $gateHash = Get-SelfTestSha256 -Path (
        Join-Path $fixture 'scripts\qualification-gates.json'
    )
    $gateSpec = Get-Content -LiteralPath (
        Join-Path $fixture 'scripts\qualification-gates.json'
    ) -Raw | ConvertFrom-Json
    $metadata = [ordered]@{
        schema_version = 2
        product = 'AgenTerm'
        version = '0.1.7'
        git_commit = $head
        git_dirty = $false
        cargo_lock_sha256 = $cargoHash
        artifact_manifest_sha256 = $artifactHash
        executables = $executableRecords
    }
    $metadata | ConvertTo-Json -Depth 6 |
        Set-Content -LiteralPath (Join-Path $fixture 'dist\agenterm.json')
    $receipt = [ordered]@{
        schema_version = 1
        product = 'AgenTerm'
        release = $true
        stress_included = $true
        gate_manifest = @{ sha256 = $gateHash }
        provenance = [ordered]@{
            git_head = $head
            source_dirty = $false
            cargo_lock_sha256 = $cargoHash
            artifact_manifest_sha256 = $artifactHash
            sbom_sha256 = $sbomHash
            executables = @(
                $executableRecords | ForEach-Object {
                    [ordered]@{
                        name = $_.name
                        sha256 = $_.sha256
                    }
                }
            )
        }
        gates = @(
            $gateSpec.required_gates | ForEach-Object {
                @{
                    id = [string]$_.id
                    status = 'passed'
                }
            }
        )
    }
    $receiptPath = Join-Path $fixture 'target\qualification\receipt.json'
    $receipt | ConvertTo-Json -Depth 7 |
        Set-Content -LiteralPath $receiptPath

    & (Join-Path $PSScriptRoot 'package-qualified.ps1') `
        -RepoRoot $fixture `
        -ReceiptPath 'target\qualification\receipt.json' `
        -OutputDirectory 'target\qualification\package-dry-run'
    $result = Get-ChildItem (
        Join-Path $fixture 'target\qualification\package-dry-run'
    ) -Recurse -File -Filter 'dry-run-result.json'
    if (@($result).Count -ne 1) {
        throw 'Package qualification did not emit exactly one dry-run result.'
    }
    $resultObject = Get-Content $result.FullName -Raw | ConvertFrom-Json
    $archivePath = Join-Path $result.Directory.FullName $resultObject.archive
    if (-not [bool]$resultObject.dry_run -or
        -not (Test-Path -LiteralPath $archivePath) -or
        $resultObject.archive_sha256 -ne
            (Get-SelfTestSha256 -Path $archivePath)) {
        throw 'Package qualification dry-run result was incomplete.'
    }
    $archive = [IO.Compression.ZipFile]::OpenRead($archivePath)
    try {
        $archiveNames = @($archive.Entries | ForEach-Object { $_.FullName })
    }
    finally {
        $archive.Dispose()
    }
    $expectedArchiveNames = @(
        @($artifactSpec.executables.name)
        'agenterm.json'
        'artifacts.json'
        'qualification-receipt.json'
        'agenterm-sbom.spdx.json'
        'LICENSE-APACHE'
        'LICENSE-MIT'
        'THIRD_PARTY_NOTICES.md'
        'package-manifest.json'
    ) | Sort-Object
    if (($expectedArchiveNames -join "`n") -ne
        (@($archiveNames | Sort-Object) -join "`n")) {
        throw 'Dry-run archive did not contain the exact qualified payload.'
    }

    $receipt.provenance.cargo_lock_sha256 = ('0' * 64)
    $receipt | ConvertTo-Json -Depth 7 |
        Set-Content -LiteralPath $receiptPath
    Assert-PackageRejected -Pattern 'Cargo.lock hash mismatch' -Action {
        & (Join-Path $PSScriptRoot 'package-qualified.ps1') `
            -RepoRoot $fixture `
            -ReceiptPath 'target\qualification\receipt.json' `
            -OutputDirectory 'target\qualification\rejected-cargo'
    }
    $receipt.provenance.cargo_lock_sha256 = $cargoHash
    $receipt.provenance.artifact_manifest_sha256 = ('0' * 64)
    $receipt | ConvertTo-Json -Depth 7 |
        Set-Content -LiteralPath $receiptPath
    Assert-PackageRejected -Pattern 'artifact manifest hash mismatch' -Action {
        & (Join-Path $PSScriptRoot 'package-qualified.ps1') `
            -RepoRoot $fixture `
            -ReceiptPath 'target\qualification\receipt.json' `
            -OutputDirectory 'target\qualification\rejected-manifest'
    }
    $receipt.provenance.artifact_manifest_sha256 = $artifactHash
    $receipt | ConvertTo-Json -Depth 7 |
        Set-Content -LiteralPath $receiptPath

    $firstExecutable = [string]$artifactSpec.executables[0].name
    Add-Content -LiteralPath (
        Join-Path (Join-Path $fixture 'dist') $firstExecutable
    ) -Value 'tampered'
    Assert-PackageRejected -Pattern 'SHA-256 mismatch' -Action {
        & (Join-Path $PSScriptRoot 'package-qualified.ps1') `
            -RepoRoot $fixture `
            -ReceiptPath 'target\qualification\receipt.json' `
            -OutputDirectory 'target\qualification\rejected-output'
    }
    $receipt.provenance.git_head = ('0' * 40)
    $receipt | ConvertTo-Json -Depth 7 |
        Set-Content -LiteralPath $receiptPath
    Assert-PackageRejected -Pattern 'HEAD does not match' -Action {
        & (Join-Path $PSScriptRoot 'package-qualified.ps1') `
            -RepoRoot $fixture `
            -ReceiptPath 'target\qualification\receipt.json' `
            -OutputDirectory 'target\qualification\rejected-head'
    }

    $source = Get-Content -LiteralPath (
        Join-Path $PSScriptRoot 'package-qualified.ps1'
    ) -Raw
    foreach ($forbidden in @(
        'build\.bat', 'cargo\s+build', 'git\s+tag', 'git\s+push',
        'gh\s+release', 'release\.ps1'
    )) {
        if ($source -match $forbidden) {
            throw "Offline packager contains forbidden action: $forbidden"
        }
    }
}
finally {
    $resolved = [IO.Path]::GetFullPath($fixture)
    $prefix = [IO.Path]::GetFullPath($ownedRoot).TrimEnd(
        [IO.Path]::DirectorySeparatorChar
    ) + [IO.Path]::DirectorySeparatorChar
    if (-not $resolved.StartsWith(
            $prefix, [StringComparison]::OrdinalIgnoreCase
        )) {
        throw "Refusing to remove package self-test path: $resolved"
    }
    if (Test-Path -LiteralPath $resolved) {
        foreach ($file in @(
            [IO.Directory]::EnumerateFiles(
                $resolved, '*', [IO.SearchOption]::AllDirectories
            )
        )) {
            [IO.File]::SetAttributes($file, [IO.FileAttributes]::Normal)
        }
        [IO.Directory]::Delete($resolved, $true)
    }
}

Write-Host 'PASS: qualified package dry-run self-test'
exit 0
