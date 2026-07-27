param(
    [string]$GuiExe = (Join-Path $PSScriptRoot '..\dist\agenterm.exe'),
    [string]$CliExe = (Join-Path $PSScriptRoot '..\dist\agenterm-cli.exe')
)

$ErrorActionPreference = 'Stop'
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$bundleRoot = [IO.Path]::GetFullPath(
    (Join-Path $repoRoot 'target\smoke\test-runs')
)
. (Join-Path $PSScriptRoot 'TestHarness.ps1')

$GuiExe = [IO.Path]::GetFullPath($GuiExe)
$CliExe = [IO.Path]::GetFullPath($CliExe)
foreach ($path in @($GuiExe, $CliExe)) {
    if (-not (Test-Path -LiteralPath $path)) {
        throw "AgenTerm executable not found: $path"
    }
}

$retainedBundles = [Collections.Generic.List[string]]::new()

function Get-RetainedBundlePaths {
    if (-not (Test-Path -LiteralPath $bundleRoot)) {
        return @()
    }
    return @(
        Get-ChildItem -LiteralPath $bundleRoot -Directory |
            ForEach-Object { [IO.Path]::GetFullPath($_.FullName) }
    )
}

function Get-OnlyNewBundle {
    param(
        [Parameter(Mandatory = $true)]
        [AllowEmptyCollection()]
        [string[]]$Before,
        [Parameter(Mandatory = $true)][string]$ExpectedSuite
    )

    $after = @(Get-RetainedBundlePaths)
    $newBundles = @($after | Where-Object { $Before -notcontains $_ })
    if ($newBundles.Count -ne 1) {
        throw (
            "$ExpectedSuite probe retained $($newBundles.Count) new bundles; " +
            'expected exactly one.'
        )
    }
    $bundle = $newBundles[0]
    if ([IO.Path]::GetFileName($bundle) -notlike "$ExpectedSuite-*") {
        throw "$ExpectedSuite probe retained an unexpected bundle: $bundle"
    }
    $retainedBundles.Add($bundle)
    return $bundle
}

function Remove-SelfTestBundle {
    param([Parameter(Mandatory = $true)][string]$Path)

    $resolved = [IO.Path]::GetFullPath($Path)
    $prefix = $bundleRoot.TrimEnd(
        [IO.Path]::DirectorySeparatorChar,
        [IO.Path]::AltDirectorySeparatorChar
    ) + [IO.Path]::DirectorySeparatorChar
    if (-not $resolved.StartsWith(
            $prefix, [StringComparison]::OrdinalIgnoreCase
        )) {
        throw "Refusing to remove bundle outside the owned smoke root: $resolved"
    }
    if (Test-Path -LiteralPath $resolved) {
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
    if (Test-Path -LiteralPath $resolved) {
        throw "Self-test bundle remained after cleanup: $resolved"
    }
}

function Assert-BoundedFailureBundle {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Suite,
        [Parameter(Mandatory = $true)][string]$MarkerKind
    )

    $manifestPath = Join-Path $Path 'manifest.json'
    $cleanupPath = Join-Path $Path 'cleanup.json'
    foreach ($required in @($manifestPath, $cleanupPath)) {
        if (-not (Test-Path -LiteralPath $required)) {
            throw "$Suite bundle omitted required file: $required"
        }
    }
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    $cleanup = Get-Content -LiteralPath $cleanupPath -Raw | ConvertFrom-Json
    $marker = "INTERNAL_FAILURE_BUNDLE_PROBE:${MarkerKind}:$($manifest.run_id)"
    $markerMatches = [regex]::Matches(
        [string]$manifest.failure, [regex]::Escape($marker)
    )
    if ($manifest.schema_version -ne 1 -or
        $manifest.suite -ne $Suite -or
        $markerMatches.Count -ne 1) {
        throw "$Suite bundle did not retain its one original failure marker."
    }
    if ($manifest.privacy.command_arguments -ne
            'known content-bearing arguments redacted' -or
        $manifest.privacy.output_limit_bytes -ne 65536 -or
        $manifest.privacy.pane_capture -ne 'disabled for this suite' -or
        @($manifest.diagnostics) -contains 'capture-pane.txt') {
        throw "$Suite bundle violated its bounded diagnostic privacy policy."
    }

    $failureDirectory = [IO.Path]::GetFullPath((Join-Path $Path 'failure'))
    $failurePrefix = $failureDirectory.TrimEnd(
        [IO.Path]::DirectorySeparatorChar,
        [IO.Path]::AltDirectorySeparatorChar
    ) + [IO.Path]::DirectorySeparatorChar
    $diagnosticNames = @($manifest.diagnostics)
    if (@($diagnosticNames | Select-Object -Unique).Count -ne
        $diagnosticNames.Count) {
        throw "$Suite bundle listed duplicate diagnostics."
    }
    foreach ($name in $diagnosticNames) {
        if ([IO.Path]::GetFileName([string]$name) -ne [string]$name) {
            throw "$Suite bundle listed a non-local diagnostic path: $name"
        }
        $diagnosticPath = [IO.Path]::GetFullPath(
            (Join-Path $failureDirectory ([string]$name))
        )
        if (-not $diagnosticPath.StartsWith(
                $failurePrefix, [StringComparison]::OrdinalIgnoreCase
            ) -or -not (Test-Path -LiteralPath $diagnosticPath)) {
            throw "$Suite bundle listed an invalid diagnostic: $name"
        }
        if ((Get-Item -LiteralPath $diagnosticPath).Length -gt (65536 + 256)) {
            throw "$Suite diagnostic exceeded its bounded output contract: $name"
        }
    }

    $commandLog = Join-Path $Path ([string]$manifest.command_log)
    if (-not (Test-Path -LiteralPath $commandLog) -or
        (Get-Item -LiteralPath $commandLog).Length -gt (524288 + 256)) {
        throw "$Suite bundle command log was missing or unbounded."
    }
    if (-not $cleanup.orphan_free -or
        @($cleanup.remaining_pids).Count -ne 0 -or
        @($cleanup.remaining_windows).Count -ne 0 -or
        @($cleanup.remaining_registrations).Count -ne 0) {
        throw "$Suite bundle cleanup was not orphan-free."
    }

    if ($Suite -eq 'theme') {
        $snapshotPath = Join-Path $failureDirectory 'ui-snapshot.txt'
        $snapshot = Get-Content -LiteralPath $snapshotPath -Raw
        if (-not $snapshot.StartsWith('exit_code=0') -or
            -not $snapshot.Contains('"protocol_version": 1') -or
            -not $snapshot.Contains('"window"')) {
            throw 'GUI probe did not retain a successful real-window diagnostic.'
        }
    }
    elseif ($Suite -eq 'script') {
        $commands = Get-Content -LiteralPath $commandLog -Raw
        if ($commands -notmatch '(?ms)command=.* script <content> <content>.*?output:\s*42') {
            throw 'Script probe did not retain evidence of a successful worker result.'
        }
        $retainedText = @(
            [string]$manifest.failure
            $commands
            $diagnosticNames | ForEach-Object {
                Get-Content -LiteralPath (
                    Join-Path $failureDirectory ([string]$_)
                ) -Raw
            }
        ) -join "`n"
        foreach ($forbidden in @('AUDIT_ENV_SECRET', '40 + 2')) {
            if ($retainedText.Contains($forbidden)) {
                throw "Script bundle exposed private worker input: $forbidden"
            }
        }
    }
}

function Invoke-ExternalProbe {
    param(
        [Parameter(Mandatory = $true)][string]$ScriptPath,
        [Parameter(Mandatory = $true)][string[]]$Arguments,
        [Parameter(Mandatory = $true)][string]$Suite
    )

    $before = @(Get-RetainedBundlePaths)
    $shellPath = (Get-Process -Id $PID).Path
    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $shellPath
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $startInfo.ArgumentList.Add('-NoProfile')
    $startInfo.ArgumentList.Add('-File')
    $startInfo.ArgumentList.Add($ScriptPath)
    foreach ($argument in $Arguments) {
        $startInfo.ArgumentList.Add($argument)
    }
    $process = [Diagnostics.Process]::Start($startInfo)
    try {
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(45000)) {
            $process.Kill($true)
            throw "$Suite failure-bundle probe exceeded its bounded deadline."
        }
        $stdout = $stdoutTask.GetAwaiter().GetResult()
        $stderr = $stderrTask.GetAwaiter().GetResult()
        if ($process.ExitCode -eq 0) {
            throw "$Suite failure-bundle probe unexpectedly succeeded."
        }
    }
    finally {
        $process.Dispose()
    }
    $bundle = Get-OnlyNewBundle -Before $before -ExpectedSuite $Suite
    if ("$stdout`n$stderr" -notmatch '(?m)^FAILURE BUNDLE ') {
        throw "$Suite probe did not report its retained failure bundle."
    }
    return $bundle
}

try {
    Write-Host 'STEP CLI failure bundle'
    $before = @(Get-RetainedBundlePaths)
    $context = New-SmokeRunContext -Suite 'diagnostic-cli-probe' `
        -Executable $CliExe
    $marker = "INTERNAL_FAILURE_BUNDLE_PROBE:cli:$($context.RunId)"
    $failure = $null
    try {
        Invoke-SmokeCli -Context $context `
            -Arguments @('__diagnostic_bundle_probe_invalid__') | Out-Null
    }
    catch {
        $failure = [InvalidOperationException]::new(
            "$marker`n$(($_ | Out-String).Trim())"
        )
    }
    finally {
        Complete-SmokeRun -Context $context -Succeeded $false `
            -FailureRecord $failure
    }
    if ($null -eq $failure) {
        throw 'CLI failure-bundle probe did not fail as expected.'
    }
    $cliBundle = Get-OnlyNewBundle -Before $before `
        -ExpectedSuite 'diagnostic-cli-probe'
    Assert-BoundedFailureBundle -Path $cliBundle `
        -Suite 'diagnostic-cli-probe' -MarkerKind 'cli'
    Remove-SelfTestBundle -Path $cliBundle

    Write-Host 'STEP GUI failure bundle'
    $guiBundle = Invoke-ExternalProbe `
        -ScriptPath (Join-Path $PSScriptRoot 'theme_smoke.ps1') `
        -Arguments @(
            '-GuiExe', $GuiExe, '-CliExe', $CliExe,
            '-InternalFailureBundleProbe'
        ) -Suite 'theme'
    Assert-BoundedFailureBundle -Path $guiBundle -Suite 'theme' -MarkerKind 'gui'
    Remove-SelfTestBundle -Path $guiBundle

    Write-Host 'STEP script-worker failure bundle'
    $scriptBundle = Invoke-ExternalProbe `
        -ScriptPath (Join-Path $PSScriptRoot 'script_smoke.ps1') `
        -Arguments @('-Exe', $CliExe, '-InternalFailureBundleProbe') `
        -Suite 'script'
    Assert-BoundedFailureBundle -Path $scriptBundle `
        -Suite 'script' -MarkerKind 'script'
    Remove-SelfTestBundle -Path $scriptBundle

    Write-Host 'PASS: CLI, GUI, and script failure bundles are bounded and orphan-free'
}
finally {
    foreach ($bundle in $retainedBundles) {
        Remove-SelfTestBundle -Path $bundle
    }
}
