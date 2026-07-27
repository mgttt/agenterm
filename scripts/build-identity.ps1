[CmdletBinding()]
param(
    [ValidateSet('dev', 'release-fast', 'release')]
    [string]$Profile,
    [string]$OutputPath,
    [switch]$SelfTest
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Get-AgenTermSha256 {
    param([Parameter(Mandatory)][string]$Path)

    $resolved = (Resolve-Path -LiteralPath $Path).Path
    (Get-FileHash -Algorithm SHA256 -LiteralPath $resolved).Hash.ToLowerInvariant()
}

function Assert-HexValue {
    param(
        [Parameter(Mandatory)][string]$Name,
        [Parameter(Mandatory)][string]$Value,
        [Parameter(Mandatory)][int[]]$Lengths
    )

    if ($Lengths -notcontains $Value.Length -or $Value -notmatch '^[0-9a-f]+$') {
        throw "$Name is not a valid lowercase hexadecimal identity."
    }
}

function ConvertTo-BuildEnvironment {
    param(
        [Parameter(Mandatory)][string]$GitCommit,
        [Parameter(Mandatory)][bool]$GitDirty,
        [Parameter(Mandatory)][string]$CargoLockSha256,
        [Parameter(Mandatory)][string]$ArtifactManifestSha256,
        [Parameter(Mandatory)][string]$BuildProfile
    )

    Assert-HexValue -Name 'Git commit' -Value $GitCommit -Lengths @(40, 64)
    Assert-HexValue -Name 'Cargo.lock SHA256' -Value $CargoLockSha256 -Lengths @(64)
    Assert-HexValue `
        -Name 'scripts/artifacts.json SHA256' `
        -Value $ArtifactManifestSha256 `
        -Lengths @(64)
    if ($BuildProfile -notin @('dev', 'release-fast', 'release')) {
        throw "Unsupported build profile '$BuildProfile'."
    }

    @(
        'set "AGENTERM_BUILD_IDENTITY_VERSION=1"'
        "set `"AGENTERM_BUILD_GIT_COMMIT=$GitCommit`""
        "set `"AGENTERM_BUILD_GIT_DIRTY=$($GitDirty.ToString().ToLowerInvariant())`""
        "set `"AGENTERM_BUILD_CARGO_LOCK_SHA256=$CargoLockSha256`""
        "set `"AGENTERM_BUILD_ARTIFACT_MANIFEST_SHA256=$ArtifactManifestSha256`""
        "set `"AGENTERM_BUILD_PROFILE=$BuildProfile`""
    )
}

function Invoke-BuildIdentitySelfTest {
    $temporaryRoot = Join-Path (
        [IO.Path]::GetTempPath()
    ) "agenterm-build-identity-selftest-$PID-$([Guid]::NewGuid().ToString('N'))"
    [IO.Directory]::CreateDirectory($temporaryRoot) | Out-Null
    try {
        $samplePath = Join-Path $temporaryRoot 'sample.txt'
        [IO.File]::WriteAllText(
            $samplePath,
            'abc',
            [Text.UTF8Encoding]::new($false)
        )
        $actualHash = Get-AgenTermSha256 -Path $samplePath
        $expectedHash = (
            'ba7816bf8f01cfea414140de5dae2223' +
            'b00361a396177a9cb410ff61f20015ad'
        )
        if ($actualHash -ne $expectedHash) {
            throw 'SHA256 self-test returned an unexpected digest.'
        }

        $lines = ConvertTo-BuildEnvironment `
            -GitCommit ('a' * 40) `
            -GitDirty $true `
            -CargoLockSha256 ('b' * 64) `
            -ArtifactManifestSha256 ('c' * 64) `
            -BuildProfile 'release-fast'
        if ($lines.Count -ne 6 -or
            $lines[0] -ne 'set "AGENTERM_BUILD_IDENTITY_VERSION=1"' -or
            $lines[2] -ne 'set "AGENTERM_BUILD_GIT_DIRTY=true"' -or
            $lines[5] -ne 'set "AGENTERM_BUILD_PROFILE=release-fast"') {
            throw 'Batch environment self-test returned an invalid contract.'
        }

        $invalidRejected = $false
        try {
            ConvertTo-BuildEnvironment `
                -GitCommit 'not-a-commit' `
                -GitDirty $false `
                -CargoLockSha256 ('b' * 64) `
                -ArtifactManifestSha256 ('c' * 64) `
                -BuildProfile 'dev' | Out-Null
        }
        catch {
            $invalidRejected = $true
        }
        if (-not $invalidRejected) {
            throw 'Invalid identity self-test input was accepted.'
        }
    }
    finally {
        Remove-Item -LiteralPath $temporaryRoot -Recurse -Force
    }
    Write-Output 'build identity self-test passed'
}

if ($SelfTest) {
    Invoke-BuildIdentitySelfTest
    exit 0
}

if ([string]::IsNullOrWhiteSpace($Profile)) {
    throw '-Profile is required unless -SelfTest is used.'
}
if ([string]::IsNullOrWhiteSpace($OutputPath)) {
    throw '-OutputPath is required unless -SelfTest is used.'
}

$repoRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
$gitCommit = (& git -C $repoRoot rev-parse --verify HEAD)
if ($LASTEXITCODE -ne 0) {
    throw 'Unable to resolve the full Git commit for the build.'
}
$gitCommit = ([string]$gitCommit).Trim().ToLowerInvariant()
Assert-HexValue -Name 'Git commit' -Value $gitCommit -Lengths @(40, 64)

$gitStatus = (& git -C $repoRoot status --porcelain=v1 --untracked-files=normal)
if ($LASTEXITCODE -ne 0) {
    throw 'Unable to determine whether the build source tree is dirty.'
}
$gitDirty = @($gitStatus).Count -gt 0

$cargoLockSha256 = Get-AgenTermSha256 -Path (Join-Path $repoRoot 'Cargo.lock')
$artifactManifestSha256 = Get-AgenTermSha256 -Path (
    Join-Path $repoRoot 'scripts\artifacts.json'
)
$environmentLines = ConvertTo-BuildEnvironment `
    -GitCommit $gitCommit `
    -GitDirty $gitDirty `
    -CargoLockSha256 $cargoLockSha256 `
    -ArtifactManifestSha256 $artifactManifestSha256 `
    -BuildProfile $Profile

$outputFullPath = [IO.Path]::GetFullPath($OutputPath)
$outputDirectory = [IO.Path]::GetDirectoryName($outputFullPath)
if ([string]::IsNullOrWhiteSpace($outputDirectory) -or
    -not [IO.Directory]::Exists($outputDirectory)) {
    throw 'Build identity output directory does not exist.'
}
[IO.File]::WriteAllLines(
    $outputFullPath,
    $environmentLines,
    [Text.UTF8Encoding]::new($false)
)
