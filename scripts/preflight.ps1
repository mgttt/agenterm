param(
    [string]$RepoRoot = (Join-Path $PSScriptRoot '..'),
    [string]$OutputPath = 'target\preflight\preflight.json',
    [switch]$Quiet
)

$ErrorActionPreference = 'Stop'
$watch = [Diagnostics.Stopwatch]::StartNew()
$repo = [IO.Path]::GetFullPath($RepoRoot)
$output = if ([IO.Path]::IsPathRooted($OutputPath)) {
    [IO.Path]::GetFullPath($OutputPath)
} else {
    [IO.Path]::GetFullPath((Join-Path $repo $OutputPath))
}
$gates = [Collections.Generic.List[object]]::new()

function Invoke-PreflightGate {
    param(
        [Parameter(Mandatory = $true)][string]$Id,
        [Parameter(Mandatory = $true)][scriptblock]$Check
    )
    try {
        $detail = (& $Check) -join '; '
        $script:gates.Add([ordered]@{
            id = $Id
            passed = $true
            detail = $detail
        })
    }
    catch {
        $script:gates.Add([ordered]@{
            id = $Id
            passed = $false
            detail = ($_.Exception.Message -replace '[\r\n]+', ' ')
        })
    }
}

function Get-RedactedRemoteUrl {
    param([Parameter(Mandatory = $true)][string]$Url)
    if ($Url -match '^(?<scheme>https?://)(?<userinfo>[^/@]+@)(?<rest>.+)$') {
        return "$($Matches.scheme)<redacted>@$($Matches.rest)"
    }
    return $Url
}

function Get-NormalizedTextFile {
    param([Parameter(Mandatory = $true)][string]$Path)
    return (Get-Content -LiteralPath $Path -Raw) -replace "\r\n?", "`n"
}

Invoke-PreflightGate -Id 'branch-main' -Check {
    $branchLines = @(& git -C $repo branch --show-current 2>&1)
    if ($LASTEXITCODE -ne 0) {
        throw "git branch failed: $($branchLines -join ' ')"
    }
    $script:branch = ([string]$branchLines[0]).Trim()
    if ($script:branch -ne 'main') {
        throw "Expected branch main, found '$($script:branch)'."
    }
    'main'
}

Invoke-PreflightGate -Id 'full-head' -Check {
    $headLines = @(& git -C $repo rev-parse HEAD 2>&1)
    if ($LASTEXITCODE -ne 0) {
        throw "git rev-parse failed: $($headLines -join ' ')"
    }
    $script:head = ([string]$headLines[0]).Trim().ToLowerInvariant()
    if ($script:head -notmatch '^[0-9a-f]{40}$') {
        throw "HEAD is not a full 40-character commit hash: $($script:head)"
    }
    $script:head
}

Invoke-PreflightGate -Id 'clean-tree' -Check {
    $status = @(& git -C $repo status --porcelain --untracked-files=normal 2>&1)
    if ($LASTEXITCODE -ne 0) {
        throw "git status failed: $($status -join ' ')"
    }
    if ($status.Count -ne 0) {
        throw "Working tree is dirty ($($status.Count) path(s))."
    }
    'clean'
}

Invoke-PreflightGate -Id 'cargo-package' -Check {
    $cargoPath = Join-Path $repo 'Cargo.toml'
    $cargo = Get-NormalizedTextFile -Path $cargoPath
    $package = [regex]::Match($cargo, '(?ms)^\[package\](?<body>.*?)(?=^\[|\z)')
    if (-not $package.Success) {
        throw 'Cargo.toml has no [package] table.'
    }
    $body = $package.Groups['body'].Value
    $name = [regex]::Match($body, '(?m)^name\s*=\s*"(?<v>[^"]+)"$')
    $version = [regex]::Match($body, '(?m)^version\s*=\s*"(?<v>[^"]+)"$')
    $rustVersion = [regex]::Match(
        $body, '(?m)^rust-version\s*=\s*"(?<v>[^"]+)"$'
    )
    if (-not $name.Success -or $name.Groups['v'].Value -ne 'agenterm' -or
        -not $version.Success -or
        $version.Groups['v'].Value -notmatch
            '^\d+\.\d+\.\d+([+-][0-9A-Za-z.-]+)?$' -or
        -not $rustVersion.Success -or
        $rustVersion.Groups['v'].Value -notmatch '^\d+\.\d+$') {
        throw 'Cargo package name, version, or rust-version is invalid.'
    }
    $script:version = $version.Groups['v'].Value
    $script:rustVersion = $rustVersion.Groups['v'].Value
    "version=$($script:version) rust=$($script:rustVersion)"
}

Invoke-PreflightGate -Id 'cargo-lock' -Check {
    $lock = Get-NormalizedTextFile -Path (Join-Path $repo 'Cargo.lock')
    if ($lock -notmatch '(?m)^version\s*=\s*4$') {
        throw 'Cargo.lock is not lockfile version 4.'
    }
    $rootPackage = [regex]::Match(
        $lock,
        '(?ms)^\[\[package\]\]\s*name\s*=\s*"agenterm"\s*' +
        'version\s*=\s*"(?<version>[^"]+)"'
    )
    if (-not $rootPackage.Success -or
        $rootPackage.Groups['version'].Value -ne $script:version) {
        throw 'Cargo.lock AgenTerm version does not match Cargo.toml.'
    }
    $checksums = [regex]::Matches(
        $lock, '(?m)^checksum\s*=\s*"(?<hash>[^"]+)"$'
    )
    if ($checksums.Count -eq 0) {
        throw 'Cargo.lock contains no registry checksums.'
    }
    $invalid = @($checksums | Where-Object {
        $_.Groups['hash'].Value -notmatch '^[0-9a-f]{64}$'
    })
    if ($invalid.Count -ne 0) {
        throw "Cargo.lock contains $($invalid.Count) invalid checksum hash(es)."
    }
    "packages=$(([regex]::Matches($lock, '(?m)^\[\[package\]\]$')).Count) checksums=$($checksums.Count)"
}

Invoke-PreflightGate -Id 'toolchain' -Check {
    $toolchain = Get-NormalizedTextFile -Path (
        Join-Path $repo 'rust-toolchain.toml'
    )
    $channel = [regex]::Match(
        $toolchain, '(?m)^channel\s*=\s*"(?<v>\d+\.\d+\.\d+)"$'
    )
    if (-not $channel.Success -or
        -not $toolchain.Contains('profile = "minimal"') -or
        -not $toolchain.Contains('"clippy"') -or
        -not $toolchain.Contains('"rustfmt"') -or
        $toolchain -match '(?m)^\s*targets\s*=') {
        throw 'rust-toolchain.toml is incomplete or invalid.'
    }
    $channelPrefix = $channel.Groups['v'].Value -replace '\.\d+$', ''
    if ($channelPrefix -ne $script:rustVersion) {
        throw 'Cargo rust-version and pinned toolchain channel disagree.'
    }
    "channel=$($channel.Groups['v'].Value) targets=host-only"
}

Invoke-PreflightGate -Id 'internal-version-policy' -Check {
    if ($script:version -ne '0.1.7') {
        return "not-applicable version=$($script:version)"
    }
    $tags = @(& git -C $repo tag --list v0.1.7 2>&1)
    if ($LASTEXITCODE -ne 0 -or $tags.Count -ne 0) {
        throw 'Internal-only v0.1.7 must have no local v0.1.7 tag.'
    }
    $release = Get-Content -LiteralPath (Join-Path $repo 'release.ps1') -Raw
    $workflow = Get-Content -LiteralPath (
        Join-Path $repo '.github\workflows\release.yml'
    ) -Raw
    if ($release -notmatch "version\s+-eq\s+'0\.1\.7'" -or
        $workflow -notmatch
            'expected\s*(?:-eq|==)\s*["'']v0\.1\.7["'']') {
        throw 'Internal v0.1.7 publication rejection policy is missing.'
    }
    'internal-only untagged'
}

Invoke-PreflightGate -Id 'remote-config' -Check {
    $lines = @(
        & git -C $repo config --local --get-regexp `
            '^remote\..*\.(url|pushurl)$' 2>&1
    )
    if ($LASTEXITCODE -ne 0 -or $lines.Count -eq 0) {
        throw 'No local Git remote URL is configured.'
    }
    $script:remotes = @(
        foreach ($line in $lines) {
            if ([string]$line -notmatch
                '^remote\.(?<name>[^.]+)\.(?<kind>url|pushurl)\s+(?<url>.+)$') {
                throw 'Could not parse a local Git remote entry.'
            }
            $url = $Matches.url
            [ordered]@{
                name = $Matches.name
                kind = $Matches.kind
                url = Get-RedactedRemoteUrl -Url $url
                embedded_credentials = (
                    $url -match '^https?://[^/@]+@'
                )
            }
        }
    )
    if (@($script:remotes | Where-Object name -eq 'origin').Count -eq 0) {
        throw 'Local Git configuration has no origin remote.'
    }
    "entries=$($script:remotes.Count)"
}

Invoke-PreflightGate -Id 'artifact-manifest' -Check {
    $manifest = Get-Content -LiteralPath (
        Join-Path $repo 'scripts\artifacts.json'
    ) -Raw | ConvertFrom-Json
    $names = @($manifest.executables.name | ForEach-Object { [string]$_ })
    $expectedNames = @(
        'agenterm.exe'
        'agenterm-server.exe'
        'agenterm-cli.exe'
        'agenterm-mux.exe'
        'agenterm-script.exe'
    )
    if ($manifest.schema_version -ne 2 -or
        (Compare-Object $expectedNames $names) -or
        @($names | Where-Object {
            $_ -notmatch '^agenterm(?:-[a-z]+)?\.exe$'
        }).Count -ne 0 -or
        @($manifest.executables | Where-Object {
            [int]$_.pe_subsystem -notin @(2, 3) -or
            [uint64]$_.release_budget_bytes -eq 0
        }).Count -ne 0) {
        throw 'Artifact manifest schema or executable set is invalid.'
    }
    "executables=$($names.Count)"
}

Invoke-PreflightGate -Id 'gate-manifest' -Check {
    $manifest = Get-Content -LiteralPath (
        Join-Path $repo 'scripts\qualification-gates.json'
    ) -Raw | ConvertFrom-Json
    $ids = @($manifest.required_gates.id | ForEach-Object { [string]$_ })
    if ($manifest.schema_version -ne 1 -or
        $manifest.receipt_schema_version -ne 1 -or
        $ids.Count -eq 0 -or
        @($ids | Sort-Object -Unique).Count -ne $ids.Count -or
        @($ids | Where-Object {
            $_ -notmatch '^[a-z0-9][a-z0-9-]+$'
        }).Count -ne 0) {
        throw 'Qualification gate manifest schema or gate IDs are invalid.'
    }
    "required_gates=$($ids.Count)"
}

$watch.Stop()
$passed = @($gates | Where-Object { -not $_.passed }).Count -eq 0
$report = [ordered]@{
    schema_version = 1
    kind = 'agenterm-read-only-preflight'
    passed = $passed
    duration_ms = [long]$watch.ElapsedMilliseconds
    repo = $repo
    branch = $branch
    head = $head
    version = $version
    remotes = @($remotes)
    gates = @($gates)
}
$parent = Split-Path -Parent $output
[IO.Directory]::CreateDirectory($parent) | Out-Null
$report | ConvertTo-Json -Depth 7 |
    Set-Content -LiteralPath $output -Encoding UTF8

if (-not $Quiet) {
    Write-Host (
        "PREFLIGHT passed=$passed duration_ms=$($report.duration_ms) " +
        "branch=$branch head=$head version=$version"
    )
    foreach ($gate in $gates) {
        $status = if ($gate.passed) { 'PASS' } else { 'FAIL' }
        Write-Host "  $status $($gate.id): $($gate.detail)"
    }
    Write-Host "PREFLIGHT JSON $output"
}
if (-not $passed) {
    throw "AgenTerm read-only preflight failed; report: $output"
}
