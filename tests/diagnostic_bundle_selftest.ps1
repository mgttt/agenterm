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
    $newBundles = @($after | Where-Object {
        $Before -notcontains $_ -and
        [IO.Path]::GetFileName($_) -like "$ExpectedSuite-*"
    })
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
        throw (
            "$Suite bundle identity/failure marker mismatch: " +
            "schema=$($manifest.schema_version) suite=$($manifest.suite) " +
            "expected_suite=$Suite marker_matches=$($markerMatches.Count) " +
            "run_id=$($manifest.run_id) " +
            "failure_length=$(([string]$manifest.failure).Length)"
        )
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
        $commandsText = Get-Content -LiteralPath $commandLog -Raw
        $commands = @($commandsText | ConvertFrom-Json)
        $successfulWorker = @(
            $commands | Where-Object {
                $_.exit_code -eq 0 -and
                @($_.arguments) -contains 'script' -and
                @($_.arguments) -contains 'eval' -and
                @($_.arguments) -contains '<content>' -and
                ([string]$_.output).Trim() -eq '42'
            }
        )
        if ($successfulWorker.Count -lt 1) {
            throw 'Script probe did not retain evidence of a successful worker result.'
        }
        $retainedText = @(
            [string]$manifest.failure
            $commandsText
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

function Start-ExternalProbe {
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
    return [pscustomobject]@{
        Before = $before
        Suite = $Suite
        Process = $process
        Stdout = $process.StandardOutput.ReadToEndAsync()
        Stderr = $process.StandardError.ReadToEndAsync()
        RequireAnnouncement = $true
    }
}

function Start-ExternalCommandProbe {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][string[]]$Arguments,
        [Parameter(Mandatory = $true)][string]$Suite
    )

    $before = @(Get-RetainedBundlePaths)
    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $Executable
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    foreach ($argument in $Arguments) {
        $startInfo.ArgumentList.Add($argument)
    }
    $process = [Diagnostics.Process]::Start($startInfo)
    return [pscustomobject]@{
        Before = $before
        Suite = $Suite
        Process = $process
        Stdout = $process.StandardOutput.ReadToEndAsync()
        Stderr = $process.StandardError.ReadToEndAsync()
        # Script Runtime currently returns a typed failure envelope and does
        # not replay captured print output after a failed invocation. The
        # independently discovered, identity-checked bundle is authoritative.
        RequireAnnouncement = $false
    }
}

function Complete-ExternalProbe {
    param([Parameter(Mandatory = $true)]$Probe)

    $process = $Probe.Process
    try {
        if (-not $process.WaitForExit(45000)) {
            $process.Kill($true)
            throw (
                "$($Probe.Suite) failure-bundle probe exceeded its bounded deadline."
            )
        }
        $stdout = $Probe.Stdout.GetAwaiter().GetResult()
        $stderr = $Probe.Stderr.GetAwaiter().GetResult()
        if ($process.ExitCode -eq 0) {
            throw "$($Probe.Suite) failure-bundle probe unexpectedly succeeded."
        }
    }
    finally {
        $process.Dispose()
        $Probe.Process = $null
    }
    $bundle = Get-OnlyNewBundle -Before $Probe.Before `
        -ExpectedSuite $Probe.Suite
    if ($Probe.RequireAnnouncement -and
        "$stdout`n$stderr" -notmatch '(?m)^FAILURE BUNDLE ') {
        throw "$($Probe.Suite) probe did not report its retained failure bundle."
    }
    return $bundle
}

$externalProbes = [Collections.Generic.List[object]]::new()
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

    Write-Host 'STEP GUI and script-worker failure bundles in parallel'
    $guiProbe = Start-ExternalCommandProbe `
        -Executable (Join-Path $PSScriptRoot '..\dist\agenterm-script.exe') `
        -Arguments @(
            'task', 'run', 'theme-smoke',
            '--manifest', (Join-Path $PSScriptRoot '..\agenterm.tasks.json'),
            '--timeout-ms', '60000',
            '--max-operations', '10000000',
            '--', '--internal-failure-bundle-probe'
        ) -Suite 'theme'
    $externalProbes.Add($guiProbe)
    $scriptProbe = Start-ExternalCommandProbe `
        -Executable (Join-Path $PSScriptRoot '..\dist\agenterm-script.exe') `
        -Arguments @(
            'task', 'run', 'script-smoke',
            '--manifest', (Join-Path $PSScriptRoot '..\agenterm.tasks.json'),
            '--timeout-ms', '120000',
            '--max-operations', '10000000',
            '--max-string-bytes', '8388608',
            '--max-output-bytes', '1048576',
            '--', '--internal-failure-bundle-probe'
        ) -Suite 'script'
    $externalProbes.Add($scriptProbe)

    $guiBundle = Complete-ExternalProbe -Probe $guiProbe
    Assert-BoundedFailureBundle -Path $guiBundle -Suite 'theme' -MarkerKind 'gui'
    Remove-SelfTestBundle -Path $guiBundle

    $scriptBundle = Complete-ExternalProbe -Probe $scriptProbe
    Assert-BoundedFailureBundle -Path $scriptBundle `
        -Suite 'script' -MarkerKind 'script'
    Remove-SelfTestBundle -Path $scriptBundle

    Write-Host 'PASS: CLI, GUI, and script failure bundles are bounded and orphan-free'
}
finally {
    foreach ($probe in $externalProbes) {
        if ($null -ne $probe.Process) {
            if (-not $probe.Process.HasExited) {
                $probe.Process.Kill($true)
                $probe.Process.WaitForExit(3000) | Out-Null
            }
            $probe.Process.Dispose()
            $probe.Process = $null
        }
    }
    foreach ($bundle in $retainedBundles) {
        Remove-SelfTestBundle -Path $bundle
    }
}
