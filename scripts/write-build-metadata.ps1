param(
    [Parameter(Mandatory = $true)]
    [string]$ManifestPath,

    [Parameter(Mandatory = $true)]
    [string]$ExecutablePath,

    [Parameter(Mandatory = $true)]
    [ValidateSet('dev', 'release')]
    [string]$Profile
)

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$cargoToml = Get-Content -LiteralPath (Join-Path $repoRoot 'Cargo.toml') -Raw
$versionMatch = [regex]::Match(
    $cargoToml,
    '(?ms)^\[package\].*?^version\s*=\s*"(?<version>[^"]+)"'
)
if (-not $versionMatch.Success) {
    throw 'Could not read the package version from Cargo.toml.'
}

$commit = $null
$previousErrorAction = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
$commitOutput = & git -C $repoRoot rev-parse --short=12 HEAD 2>$null
$commitExitCode = $LASTEXITCODE
$ErrorActionPreference = $previousErrorAction
if ($commitExitCode -eq 0) {
    $commit = ($commitOutput | Select-Object -First 1).Trim()
}

$statusOutput = & git -C $repoRoot status --porcelain 2>$null
$isDirty = $LASTEXITCODE -eq 0 -and @($statusOutput).Count -gt 0

$hostTarget = $null
$hostLine = & rustc -vV |
    Where-Object { $_ -like 'host: *' } |
    Select-Object -First 1
if ($hostLine) {
    $hostTarget = $hostLine.Substring('host: '.Length).Trim()
}

$executable = Get-Item -LiteralPath $ExecutablePath
$stream = [IO.File]::OpenRead($ExecutablePath)
try {
    $sha256 = [Security.Cryptography.SHA256]::Create()
    try {
        $hash = [BitConverter]::ToString($sha256.ComputeHash($stream)).
            Replace('-', '').
            ToLowerInvariant()
    }
    finally {
        $sha256.Dispose()
    }
}
finally {
    $stream.Dispose()
}

$manifest = [ordered]@{
    schema_version  = 1
    product         = 'AgenTerm'
    version         = $versionMatch.Groups['version'].Value
    build_time_utc  = [DateTime]::UtcNow.ToString(
        'yyyy-MM-ddTHH:mm:ssZ',
        [Globalization.CultureInfo]::InvariantCulture
    )
    git_commit      = $commit
    git_dirty       = $isDirty
    profile         = $Profile
    rust_target     = $hostTarget
    executable      = $executable.Name
    executable_size = $executable.Length
    sha256          = $hash
}

$json = $manifest | ConvertTo-Json
[IO.File]::WriteAllText(
    $ManifestPath,
    "$json`n",
    [Text.UTF8Encoding]::new($false)
)
