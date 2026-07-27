param(
    [Parameter(Mandatory = $true)]
    [string]$ManifestPath,

    [Parameter(Mandatory = $true)]
    [string]$ArtifactManifestPath,

    [Parameter(Mandatory = $true)]
    [string]$StagedDirectory,

    [Parameter(Mandatory = $true)]
    [ValidateSet('dev', 'release-fast', 'release')]
    [string]$Profile
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$stagedDirectoryPath = [IO.Path]::GetFullPath($StagedDirectory)
. (Join-Path $PSScriptRoot 'artifact-manifest.ps1')
$artifactManifest = Get-AgenTermArtifactManifest -Path $ArtifactManifestPath
$cargoToml = Get-Content -LiteralPath (Join-Path $repoRoot 'Cargo.toml') -Raw
$versionMatch = [regex]::Match(
    $cargoToml,
    '(?ms)^\[package\].*?^version\s*=\s*"(?<version>[^"]+)"'
)
if (-not $versionMatch.Success) {
    throw 'Could not read the package version from Cargo.toml.'
}

$previousErrorAction = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
$commitOutput = & git -C $repoRoot rev-parse HEAD 2>$null
$commitExitCode = $LASTEXITCODE
$ErrorActionPreference = $previousErrorAction
if ($commitExitCode -ne 0) {
    throw "Could not resolve the source Git commit (exit $commitExitCode)."
}
$commit = ($commitOutput | Select-Object -First 1).Trim()
if ($commit -notmatch '^[0-9a-fA-F]{40}$') {
    throw "Git returned an invalid source commit: $commit"
}

$previousErrorAction = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
$statusOutput = & git -C $repoRoot status --porcelain 2>$null
$statusExitCode = $LASTEXITCODE
$ErrorActionPreference = $previousErrorAction
if ($statusExitCode -ne 0) {
    throw "Could not inspect source Git status (exit $statusExitCode)."
}
$isDirty = @($statusOutput).Count -gt 0

$rustVersionLines = @(& rustc -vV)
if ($LASTEXITCODE -ne 0) {
    throw "Could not inspect the Rust compiler (exit $LASTEXITCODE)."
}
$hostLine = $rustVersionLines |
    Where-Object { $_ -like 'host: *' } |
    Select-Object -First 1
if (-not $hostLine) {
    throw 'Rust compiler metadata did not include its host target.'
}
$hostTarget = $hostLine.Substring('host: '.Length).Trim()
$rustVersion = ($rustVersionLines | Select-Object -First 1).Trim()

function Get-FileSha256 {
    param([Parameter(Mandatory = $true)][string]$Path)

    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).
        Hash.ToLowerInvariant()
}

function Get-ExecutableInfo {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,

        [Parameter(Mandatory = $true)]
        [string]$Role
    )

    $executable = Get-Item -LiteralPath $Path

    [ordered]@{
        name   = $executable.Name
        role   = $Role
        size   = $executable.Length
        sha256 = Get-FileSha256 -Path $Path
    }
}

$artifactManifestResolvedPath = (Resolve-Path -LiteralPath $ArtifactManifestPath).Path
$cargoLockPath = Join-Path $repoRoot 'Cargo.lock'
$manifest = [ordered]@{
    schema_version = 2
    product        = 'AgenTerm'
    version        = $versionMatch.Groups['version'].Value
    build_time_utc = [DateTime]::UtcNow.ToString(
        'yyyy-MM-ddTHH:mm:ssZ',
        [Globalization.CultureInfo]::InvariantCulture
    )
    git_commit     = $commit
    git_dirty      = $isDirty
    profile        = $Profile
    rust_target    = $hostTarget
    rust_version   = $rustVersion
    cargo_lock_sha256 = Get-FileSha256 -Path $cargoLockPath
    artifact_manifest_sha256 = Get-FileSha256 -Path $artifactManifestResolvedPath
    features       = @(
        'codex-launcher'
        'hierarchical-tabs'
        'mux-frontend'
        'persistent-workspace'
        'safe-scripting'
        'tab-environment'
    )
    executables    = @(
        foreach ($artifact in @($artifactManifest.executables)) {
            Get-ExecutableInfo `
                -Path (Join-Path $stagedDirectoryPath $artifact.name) `
                -Role $artifact.role
        }
    )
}

$json = $manifest | ConvertTo-Json -Depth 4
[IO.File]::WriteAllText(
    $ManifestPath,
    "$json`n",
    [Text.UTF8Encoding]::new($false)
)
