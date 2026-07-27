param(
    [ValidateRange(5, 100)][int]$Iterations = 5,
    [string]$RepoRoot = (Join-Path $PSScriptRoot '..'),
    [string]$OutputPath = 'target\preflight\benchmark.json'
)

$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath($RepoRoot)
$output = if ([IO.Path]::IsPathRooted($OutputPath)) {
    [IO.Path]::GetFullPath($OutputPath)
} else {
    [IO.Path]::GetFullPath((Join-Path $repo $OutputPath))
}
$runDirectory = Join-Path (Split-Path -Parent $output) (
    'runs-' + [Guid]::NewGuid().ToString('N')
)
[IO.Directory]::CreateDirectory($runDirectory) | Out-Null
$previousNativePreference = $PSNativeCommandUseErrorActionPreference
$PSNativeCommandUseErrorActionPreference = $false

try {
    $durations = [Collections.Generic.List[long]]::new()
    $results = @(
        for ($iteration = 1; $iteration -le $Iterations; $iteration++) {
            $runReport = Join-Path $runDirectory "$iteration.json"
            $wall = [Diagnostics.Stopwatch]::StartNew()
            $items = @(
                & (Join-Path $PSHOME 'pwsh.exe') -NoProfile -NonInteractive `
                    -File (Join-Path $PSScriptRoot 'preflight.ps1') `
                    -RepoRoot $repo -OutputPath $runReport -Quiet 2>&1
            )
            $exitCode = $LASTEXITCODE
            $wall.Stop()
            if (-not (Test-Path -LiteralPath $runReport)) {
                throw "Benchmark iteration $iteration emitted no report: $items"
            }
            $report = Get-Content -LiteralPath $runReport -Raw |
                ConvertFrom-Json
            $durations.Add([long]$wall.ElapsedMilliseconds)
            [ordered]@{
                iteration = $iteration
                passed = [bool]$report.passed
                exit_code = $exitCode
                preflight_duration_ms = [long]$report.duration_ms
                wall_duration_ms = [long]$wall.ElapsedMilliseconds
            }
        }
    )
    $sorted = @($durations | Sort-Object)
    $p95Index = [Math]::Ceiling(0.95 * $sorted.Count) - 1
    $p95 = [long]$sorted[$p95Index]
    $benchmark = [ordered]@{
        schema_version = 1
        kind = 'agenterm-read-only-preflight-benchmark'
        iterations = $Iterations
        p95_wall_duration_ms = $p95
        target_ms = 15000
        target_met = $p95 -le 15000
        excludes_network_and_interactive_auth = $true
        runs = $results
    }
    [IO.Directory]::CreateDirectory((Split-Path -Parent $output)) | Out-Null
    $benchmark | ConvertTo-Json -Depth 6 |
        Set-Content -LiteralPath $output -Encoding UTF8
    Write-Host (
        "PREFLIGHT BENCHMARK iterations=$Iterations p95_ms=$p95 " +
        "target_ms=15000 target_met=$($benchmark.target_met)"
    )
    Write-Host "PREFLIGHT BENCHMARK JSON $output"
    if (-not $benchmark.target_met) {
        throw "Preflight p95 $p95 ms exceeds the 15000 ms target."
    }
}
finally {
    $PSNativeCommandUseErrorActionPreference = $previousNativePreference
    if (Test-Path -LiteralPath $runDirectory) {
        [IO.Directory]::Delete($runDirectory, $true)
    }
}
