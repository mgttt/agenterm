$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$ownedRoot = Join-Path $repo 'target\preflight'
$selfTestRoot = Join-Path $ownedRoot (
    'selftest-' + [Guid]::NewGuid().ToString('N')
)
[IO.Directory]::CreateDirectory($selfTestRoot) | Out-Null
$previousNativePreference = $PSNativeCommandUseErrorActionPreference
$PSNativeCommandUseErrorActionPreference = $false

function New-PreflightFixture {
    param(
        [Parameter(Mandatory = $true)][string]$Name,
        [ValidateSet('clean', 'dirty', 'wrong-branch', 'bad-hash', 'bad-manifest')]
        [string]$Mode
    )
    $fixture = Join-Path $selfTestRoot $Name
    foreach ($directory in @('scripts', '.github\workflows', 'target')) {
        [IO.Directory]::CreateDirectory((Join-Path $fixture $directory)) |
            Out-Null
    }
    foreach ($path in @(
        'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml',
        'release.ps1', 'scripts\artifacts.json',
        'scripts\qualification-gates.json',
        '.github\workflows\release.yml'
    )) {
        Copy-Item -LiteralPath (Join-Path $repo $path) `
            -Destination (Join-Path $fixture $path)
    }
    @('/target/') | Set-Content -LiteralPath (Join-Path $fixture '.gitignore')
    'fixture' | Set-Content -LiteralPath (Join-Path $fixture 'README.md')
    if ($Mode -eq 'bad-hash') {
        $lockPath = Join-Path $fixture 'Cargo.lock'
        $lock = Get-Content -LiteralPath $lockPath -Raw
        $lock = [regex]::Replace(
            $lock,
            '(?m)^(checksum\s*=\s*")[^"]+(")$',
            '${1}not-a-sha256${2}',
            1
        )
        Set-Content -LiteralPath $lockPath -Value $lock
    }
    if ($Mode -eq 'bad-manifest') {
        '{ invalid json' | Set-Content -LiteralPath (
            Join-Path $fixture 'scripts\artifacts.json'
        )
    }
    & git -C $fixture init --quiet -b main
    & git -C $fixture remote add origin https://example.invalid/agenterm.git
    & git -C $fixture add .
    & git -C $fixture -c user.name=preflight-selftest `
        -c user.email=preflight-selftest.invalid commit --quiet -m fixture
    if ($LASTEXITCODE -ne 0) {
        throw "Could not commit preflight fixture: $Name"
    }
    if ($Mode -eq 'dirty') {
        Add-Content -LiteralPath (Join-Path $fixture 'README.md') -Value dirty
    }
    if ($Mode -eq 'wrong-branch') {
        & git -C $fixture switch --quiet -c feature/preflight
    }
    return $fixture
}

function Invoke-PreflightFixture {
    param(
        [Parameter(Mandatory = $true)][string]$Fixture,
        [Parameter(Mandatory = $true)][bool]$ShouldPass,
        [string]$ExpectedFailedGate = ''
    )
    $reportPath = Join-Path $Fixture 'target\preflight.json'
    $items = @(
        & (Join-Path $PSHOME 'pwsh.exe') -NoProfile -NonInteractive `
            -File (Join-Path $PSScriptRoot 'preflight.ps1') `
            -RepoRoot $Fixture -OutputPath $reportPath -Quiet 2>&1
    )
    $exitCode = $LASTEXITCODE
    if (-not (Test-Path -LiteralPath $reportPath)) {
        throw "Preflight fixture emitted no JSON report: $Fixture"
    }
    $report = Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json
    if ([bool]$report.passed -ne $ShouldPass -or
        ($ShouldPass -and $exitCode -ne 0) -or
        (-not $ShouldPass -and $exitCode -eq 0)) {
        $failures = @($report.gates | Where-Object { -not $_.passed } |
            ForEach-Object { "$($_.id)=$($_.detail)" })
        throw (
            "Unexpected preflight result for $Fixture`: " +
            "$($failures -join '; ') child=$($items -join ' ')"
        )
    }
    if (-not $ShouldPass -and
        @($report.gates | Where-Object {
            $_.id -eq $ExpectedFailedGate -and -not $_.passed
        }).Count -ne 1) {
        throw "Expected failed gate '$ExpectedFailedGate' was not reported."
    }
}

try {
    $cases = @(
        @{ name = 'clean'; mode = 'clean'; pass = $true; gate = '' }
        @{ name = 'dirty'; mode = 'dirty'; pass = $false; gate = 'clean-tree' }
        @{
            name = 'wrong-branch'
            mode = 'wrong-branch'
            pass = $false
            gate = 'branch-main'
        }
        @{
            name = 'bad-hash'
            mode = 'bad-hash'
            pass = $false
            gate = 'cargo-lock'
        }
        @{
            name = 'bad-manifest'
            mode = 'bad-manifest'
            pass = $false
            gate = 'artifact-manifest'
        }
    )
    foreach ($case in $cases) {
        $fixture = New-PreflightFixture -Name $case.name -Mode $case.mode
        Invoke-PreflightFixture -Fixture $fixture `
            -ShouldPass $case.pass -ExpectedFailedGate $case.gate
    }
    $source = Get-Content -LiteralPath (
        Join-Path $PSScriptRoot 'preflight.ps1'
    ) -Raw
    foreach ($forbidden in @(
        '\bcargo\b\s+(build|check|test|run)',
        '\brustc\b',
        'git\s+fetch',
        'git\s+push',
        'git\s+ls-remote',
        'Invoke-WebRequest',
        'Invoke-RestMethod',
        'Start-BitsTransfer'
    )) {
        if ($source -match $forbidden) {
            throw "Preflight contains forbidden active operation: $forbidden"
        }
    }
}
finally {
    $PSNativeCommandUseErrorActionPreference = $previousNativePreference
    $resolved = [IO.Path]::GetFullPath($selfTestRoot)
    $prefix = [IO.Path]::GetFullPath($ownedRoot).TrimEnd(
        [IO.Path]::DirectorySeparatorChar
    ) + [IO.Path]::DirectorySeparatorChar
    if (-not $resolved.StartsWith(
            $prefix, [StringComparison]::OrdinalIgnoreCase
        )) {
        throw "Refusing to remove preflight self-test path: $resolved"
    }
    if (Test-Path -LiteralPath $resolved) {
        foreach ($file in [IO.Directory]::EnumerateFiles(
            $resolved, '*', [IO.SearchOption]::AllDirectories
        )) {
            [IO.File]::SetAttributes($file, [IO.FileAttributes]::Normal)
        }
        [IO.Directory]::Delete($resolved, $true)
    }
}

Write-Host 'PASS: read-only preflight fixture self-test'
exit 0
