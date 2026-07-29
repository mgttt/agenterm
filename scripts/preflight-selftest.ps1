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
        [ValidateSet(
            'clean', 'crlf', 'dirty', 'wrong-branch', 'bad-hash', 'bad-manifest'
        )]
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
    if ($Mode -eq 'crlf') {
        foreach ($path in @(
            'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml'
        )) {
            $fixturePath = Join-Path $fixture $path
            $text = [IO.File]::ReadAllText($fixturePath)
            $text = ($text -replace "\r\n?", "`n") -replace "`n", "`r`n"
            [IO.File]::WriteAllText(
                $fixturePath, $text, [Text.UTF8Encoding]::new($false)
            )
        }
    }
    if ($Mode -eq 'bad-hash') {
        $lockPath = Join-Path $fixture 'Cargo.lock'
        $lock = Get-Content -LiteralPath $lockPath -Raw
        $lock = [regex]::Replace(
            $lock,
            '(?m)^(checksum\s*=\s*")[^"\r\n]+(")',
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

function Start-PreflightFixture {
    param(
        [Parameter(Mandatory = $true)][string]$Fixture,
        [Parameter(Mandatory = $true)][bool]$ShouldPass,
        [string]$ExpectedFailedGate = ''
    )
    $reportPath = Join-Path $Fixture 'target\preflight.json'
    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = Join-Path $PSHOME 'pwsh.exe'
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    foreach ($argument in @(
            '-NoProfile', '-NonInteractive',
            '-File', (Join-Path $PSScriptRoot 'preflight.ps1'),
            '-RepoRoot', $Fixture,
            '-OutputPath', $reportPath,
            '-Quiet'
        )) {
        $startInfo.ArgumentList.Add($argument)
    }
    $process = [Diagnostics.Process]::Start($startInfo)
    return [pscustomobject]@{
        Fixture = $Fixture
        ShouldPass = $ShouldPass
        ExpectedFailedGate = $ExpectedFailedGate
        ReportPath = $reportPath
        Process = $process
        Stdout = $process.StandardOutput.ReadToEndAsync()
        Stderr = $process.StandardError.ReadToEndAsync()
    }
}

function Complete-PreflightFixture {
    param([Parameter(Mandatory = $true)]$Probe)

    $process = $Probe.Process
    try {
        if (-not $process.WaitForExit(30000)) {
            $process.Kill($true)
            throw "Preflight fixture timed out: $($Probe.Fixture)"
        }
        $items = @(
            $Probe.Stdout.GetAwaiter().GetResult()
            $Probe.Stderr.GetAwaiter().GetResult()
        ) | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
        $exitCode = $process.ExitCode
    }
    finally {
        $process.Dispose()
        $Probe.Process = $null
    }
    $reportPath = $Probe.ReportPath
    if (-not (Test-Path -LiteralPath $reportPath)) {
        throw "Preflight fixture emitted no JSON report: $($Probe.Fixture)"
    }
    $report = Get-Content -LiteralPath $reportPath -Raw | ConvertFrom-Json
    if ([bool]$report.passed -ne $Probe.ShouldPass -or
        ($Probe.ShouldPass -and $exitCode -ne 0) -or
        (-not $Probe.ShouldPass -and $exitCode -eq 0)) {
        $failures = @($report.gates | Where-Object { -not $_.passed } |
            ForEach-Object { "$($_.id)=$($_.detail)" })
        throw (
            "Unexpected preflight result for $($Probe.Fixture)`: " +
            "$($failures -join '; ') child=$($items -join ' ')"
        )
    }
    if (-not $Probe.ShouldPass -and
        @($report.gates | Where-Object {
            $_.id -eq $Probe.ExpectedFailedGate -and -not $_.passed
        }).Count -ne 1) {
        throw (
            "Expected failed gate '$($Probe.ExpectedFailedGate)' was not reported."
        )
    }
}

$probes = [Collections.Generic.List[object]]::new()
try {
    $cases = @(
        @{ name = 'clean'; mode = 'clean'; pass = $true; gate = '' }
        @{ name = 'crlf'; mode = 'crlf'; pass = $true; gate = '' }
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
        $probes.Add((
            Start-PreflightFixture -Fixture $fixture `
                -ShouldPass $case.pass -ExpectedFailedGate $case.gate
        ))
    }
    foreach ($probe in $probes) {
        Complete-PreflightFixture -Probe $probe
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
    foreach ($probe in $probes) {
        if ($null -ne $probe.Process) {
            if (-not $probe.Process.HasExited) {
                $probe.Process.Kill($true)
                $probe.Process.WaitForExit(3000) | Out-Null
            }
            $probe.Process.Dispose()
            $probe.Process = $null
        }
    }
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
