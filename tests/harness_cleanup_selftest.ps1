param(
    [string]$CliExe = (Join-Path $PSScriptRoot '..\dist\agenterm-cli.exe')
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'TestHarness.ps1')

$context = New-SmokeRunContext -Suite 'harness-cleanup-selftest' `
    -Executable $CliExe
$owned = $null
$unowned = $null
$originalMarker = 'original-fixture-failure-must-survive-cleanup'
try {
    $arguments = @(
        '-NoProfile', '-NonInteractive', '-Command',
        'Start-Sleep -Seconds 60'
    )
    $owned = Start-Process -FilePath (Join-Path $PSHOME 'pwsh.exe') `
        -ArgumentList $arguments -WindowStyle Hidden -PassThru
    $unowned = Start-Process -FilePath (Join-Path $PSHOME 'pwsh.exe') `
        -ArgumentList $arguments -WindowStyle Hidden -PassThru
    Register-SmokeOwnedProcess -Context $context -Id $owned.Id `
        -Kind 'script-worker'

    $originalFailure = [InvalidOperationException]::new($originalMarker)
    Complete-SmokeRun -Context $context -Succeeded $false `
        -FailureRecord $originalFailure

    if ($null -ne (Get-Process -Id $owned.Id -ErrorAction SilentlyContinue)) {
        throw 'Harness did not remove its registered owned PID.'
    }
    if ($null -eq (Get-Process -Id $unowned.Id -ErrorAction SilentlyContinue)) {
        throw 'Harness killed an unregistered process.'
    }
    $cleanup = Get-Content -LiteralPath $context.CleanupPath -Raw |
        ConvertFrom-Json
    $manifest = Get-Content -LiteralPath $context.ManifestPath -Raw |
        ConvertFrom-Json
    if (-not [bool]$cleanup.orphan_free -or
        @($cleanup.forced_pids) -notcontains $owned.Id -or
        @($cleanup.forced_pids) -contains $unowned.Id -or
        -not ([string]$manifest.failure).Contains($originalMarker) -or
        -not [bool]$manifest.cleanup.orphan_free) {
        throw 'Harness cleanup proof or original failure preservation was incomplete.'
    }
}
finally {
    if ($null -ne $unowned -and
        $null -ne (Get-Process -Id $unowned.Id -ErrorAction SilentlyContinue)) {
        Stop-Process -Id $unowned.Id -Force
        $unowned.WaitForExit(3000) | Out-Null
    }
    if ($null -ne $owned) {
        $owned.Dispose()
    }
    if ($null -ne $unowned) {
        $unowned.Dispose()
    }
    $runPath = [IO.Path]::GetFullPath($context.RunDirectory)
    $rootPath = [IO.Path]::GetFullPath($context.OwnedRoot).TrimEnd(
        [IO.Path]::DirectorySeparatorChar
    ) + [IO.Path]::DirectorySeparatorChar
    if (-not $runPath.StartsWith(
            $rootPath, [StringComparison]::OrdinalIgnoreCase
        )) {
        throw "Refusing to remove harness self-test path: $runPath"
    }
    if (Test-Path -LiteralPath $runPath) {
        [IO.Directory]::Delete($runPath, $true)
    }
}

Write-Host 'PASS: harness cleanup ownership and orphan proof'
