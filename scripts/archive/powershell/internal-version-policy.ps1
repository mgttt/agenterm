# Archived after scripts/rhai/internal-version-policy.rhai reached parity.
# This file is retained only as a bounded rollback reference.
param(
    [string]$RepoRoot = (Join-Path $PSScriptRoot '..\..\..')
)

$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath($RepoRoot)
$cargo = Get-Content -LiteralPath (Join-Path $repo 'Cargo.toml') -Raw
$match = [regex]::Match(
    $cargo,
    '(?ms)^\[package\].*?^version\s*=\s*"(?<version>[^"]+)"'
)
if (-not $match.Success) {
    throw 'Could not read the package version from Cargo.toml.'
}
$version = $match.Groups['version'].Value
if ($version -ne '0.1.7') {
    Write-Host "PASS: $version is not governed by the internal 0.1.7 policy."
    exit 0
}

$tagOutput = @(& git -C $repo tag --list v0.1.7 2>&1)
if ($LASTEXITCODE -ne 0) {
    throw "Could not inspect local tags: $($tagOutput -join "`n")"
}
if ($tagOutput.Count -ne 0) {
    throw 'Internal-only v0.1.7 must not have a local Git tag.'
}

$releaseSource = Get-Content -LiteralPath (Join-Path $repo 'release.ps1') -Raw
$workflowSource = Get-Content -LiteralPath (
    Join-Path $repo '.github\workflows\release.yml'
) -Raw
if ($releaseSource -notmatch "version\s+-eq\s+'0\.1\.7'" -or
    $workflowSource -notmatch "expected\s+-eq\s+'v0\.1\.7'") {
    throw 'The local coordinator and tag workflow must both reject v0.1.7.'
}

Write-Host (
    'PASS: v0.1.7 is internal-only, untagged, and rejected by both ' +
    'publication entry points.'
)
