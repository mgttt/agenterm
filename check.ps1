param(
    [switch]$Release,
    [switch]$Quick,
    [switch]$SkipSmoke,
    [switch]$IncludeStress,
    [switch]$InternalQualificationDryRun,
    [string]$QualificationReceiptPath = 'target\qualification\receipt.json'
)

$ErrorActionPreference = 'Stop'
$hadNoActivateEnvironment = Test-Path Env:AGENTERM_NO_ACTIVATE
$previousNoActivateEnvironment = $env:AGENTERM_NO_ACTIVATE
Push-Location $PSScriptRoot

function Invoke-Checked {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Id,

        [Parameter(Mandatory = $true)]
        [string]$Label,

        [Parameter(Mandatory = $true)]
        [scriptblock]$Command
    )

    Write-Host "`n==> $Label"
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $outputItems = @()
    try {
        $global:LASTEXITCODE = 0
        $outputItems = @(
            & $Command *>&1 | ForEach-Object {
                Write-Host ([string]$_)
                $_
            }
        )
        $exitCode = $LASTEXITCODE
        if ($exitCode -ne 0) {
            throw "$Label failed with exit code $exitCode"
        }
        Add-AgenTermQualificationResult -Context $qualification `
            -GateId $Id -Status passed -DurationMs $watch.ElapsedMilliseconds `
            -Output $outputItems
        Write-Host "PASS: $Label ($($watch.ElapsedMilliseconds) ms)"
    }
    catch {
        $failureOutput = @($outputItems) + @($_ | Out-String)
        if (-not $qualification.Results.Contains($Id)) {
            Add-AgenTermQualificationResult -Context $qualification `
                -GateId $Id -Status failed `
                -DurationMs $watch.ElapsedMilliseconds -Output $failureOutput
        }
        throw
    }
}

function Invoke-QuickStep {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Label,

        [Parameter(Mandatory = $true)]
        [scriptblock]$Command
    )

    Write-Host "`n==> $Label"
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $global:LASTEXITCODE = 0
    & $Command
    if ($LASTEXITCODE -ne 0) {
        throw "$Label failed with exit code $LASTEXITCODE"
    }
    Write-Host "PASS: $Label ($($watch.ElapsedMilliseconds) ms)"
}

function Get-AgenTermDebugScriptWorker {
    $target = if ([string]::IsNullOrWhiteSpace(
            $env:CARGO_TARGET_DIR
        )) {
        Join-Path $PSScriptRoot 'target'
    }
    else {
        [IO.Path]::GetFullPath($env:CARGO_TARGET_DIR)
    }
    return Join-Path $target 'debug\agenterm-script.exe'
}

function Import-AgenTermDevelopmentBuildIdentity {
    $identityPath = Join-Path (
        [IO.Path]::GetTempPath()
    ) "agenterm-check-identity-$PID-$([Guid]::NewGuid().ToString('N')).cmd"
    $allowed = @(
        'AGENTERM_BUILD_IDENTITY_VERSION',
        'AGENTERM_BUILD_GIT_COMMIT',
        'AGENTERM_BUILD_GIT_DIRTY',
        'AGENTERM_BUILD_CARGO_LOCK_SHA256',
        'AGENTERM_BUILD_ARTIFACT_MANIFEST_SHA256',
        'AGENTERM_BUILD_PROFILE'
    )
    try {
        $cargoTarget = if ([string]::IsNullOrWhiteSpace(
                $env:CARGO_TARGET_DIR
            )) {
            Join-Path $PSScriptRoot 'target'
        }
        else {
            [IO.Path]::GetFullPath($env:CARGO_TARGET_DIR)
        }
        $worker = Join-Path $cargoTarget 'debug\agenterm-script.exe'
        $identityReady = $false
        if (Test-Path -LiteralPath $worker -PathType Leaf) {
            & $worker task check build-identity `
                --manifest '.\agenterm.tasks.json' *> $null
            if ($LASTEXITCODE -eq 0) {
                & $worker task run build-identity `
                    --manifest '.\agenterm.tasks.json' -- `
                    $PSScriptRoot dev $identityPath
                $identityReady = $LASTEXITCODE -eq 0
            }
        }
        if (-not $identityReady) {
            & cargo build --quiet --locked --bin agenterm-script
            if ($LASTEXITCODE -ne 0) {
                throw 'Could not bootstrap agenterm-script for build identity.'
            }
            & $worker task run build-identity `
                --manifest '.\agenterm.tasks.json' -- `
                $PSScriptRoot dev $identityPath
            if ($LASTEXITCODE -ne 0) {
                throw 'Could not freeze the development build identity.'
            }
        }
        $values = [ordered]@{}
        foreach ($line in Get-Content -LiteralPath $identityPath) {
            if ($line -notmatch '^set "([A-Z0-9_]+)=(.*)"$') {
                throw "Malformed development build identity line: $line"
            }
            $name = $Matches[1]
            if ($name -notin $allowed -or $values.Contains($name)) {
                throw "Unexpected development build identity field: $name"
            }
            $values[$name] = $Matches[2]
        }
        if ($values.Count -ne $allowed.Count) {
            throw 'Development build identity omitted required fields.'
        }
        foreach ($name in $allowed) {
            [Environment]::SetEnvironmentVariable(
                $name, [string]$values[$name], 'Process'
            )
        }
    }
    finally {
        if (Test-Path -LiteralPath $identityPath) {
            Remove-Item -LiteralPath $identityPath -Force
        }
    }
}

function Get-PeSubsystem {
    param([Parameter(Mandatory = $true)][string]$Path)

    $bytes = [IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $Path))
    $peOffset = [BitConverter]::ToInt32($bytes, 0x3c)
    [BitConverter]::ToUInt16($bytes, $peOffset + 24 + 68)
}

try {
    if ($Quick) {
        if ($Release -or $SkipSmoke -or $IncludeStress -or
            $InternalQualificationDryRun) {
            throw (
                '-Quick is a standalone development feedback lane; do not ' +
                'combine it with release, smoke, stress, or qualification options.'
            )
        }
        $quickWatch = [Diagnostics.Stopwatch]::StartNew()
        Invoke-QuickStep -Label 'repository static lint' {
            & '.\lint.ps1' -Mode Static
        }
        Invoke-QuickStep -Label 'rustfmt' {
            cargo fmt --all -- --check
        }
        Invoke-QuickStep -Label 'PRD capability alignment' {
            & '.\dist\agenterm-script.exe' task run prd-alignment `
                --manifest '.\agenterm.tasks.json' `
                --timeout-ms 10000 --max-operations 10000000 -- '.'
        }
        Invoke-QuickStep -Label 'development build identity' {
            Import-AgenTermDevelopmentBuildIdentity
        }
        Invoke-QuickStep -Label 'all-target Clippy' {
            cargo clippy --quiet --locked --all-targets -- -D warnings
        }
        Invoke-QuickStep -Label 'library unit tests' {
            cargo test --quiet --locked --lib
        }
        Write-Host (
            "`nPASS: AgenTerm quick development gate " +
            "($($quickWatch.ElapsedMilliseconds) ms)"
        )
        return
    }

    if ($InternalQualificationDryRun) {
        & '.\scripts\qualification-selftest.ps1'
        return
    }

    $receiptPath = [IO.Path]::GetFullPath($QualificationReceiptPath)
    if (Test-Path -LiteralPath $receiptPath) {
        Remove-Item -LiteralPath $receiptPath -Force
    }
    . '.\scripts\qualification.ps1'
    $fullCheckWatch = [Diagnostics.Stopwatch]::StartNew()
    $qualificationManifestPath = '.\scripts\qualification-gates.json'
    $qualification = New-AgenTermQualificationContext `
        -ManifestPath $qualificationManifestPath `
        -Release ([bool]$Release) `
        -StressIncluded ([bool]$IncludeStress)
    Invoke-Checked -Id 'preflight-selftest' `
        -Label 'read-only preflight self-test' {
        cargo test --quiet --locked --test rhai_migration `
            preflight_task_is_fail_closed_and_writes_reports_for_real_git_fixtures `
            -- --nocapture
    }
    if ($Release) {
        $scriptWorker = Get-AgenTermDebugScriptWorker
        Invoke-Checked -Id 'release-preflight' `
            -Label 'clean internal candidate preflight' {
            & $scriptWorker task run preflight `
                --manifest '.\agenterm.tasks.json' `
                --timeout-ms 10000 --max-operations 10000000 `
                -- '.' 'target\preflight\preflight.json'
        }
        Invoke-Checked -Id 'preflight-benchmark' `
            -Label 'local preflight p95 benchmark' {
            & $scriptWorker task run preflight-benchmark `
                --manifest '.\agenterm.tasks.json' `
                --timeout-ms 60000 --max-operations 10000000 `
                -- $scriptWorker '.\agenterm.tasks.json' '.' `
                'target\preflight\benchmark.json' '5'
        }
    }
    Invoke-Checked -Id 'repo-lint' -Label 'repository static lint' {
        & '.\lint.ps1' -InternalSelfTest
        if ($LASTEXITCODE -ne 0) {
            throw 'repository lint self-test failed'
        }
        & '.\lint.ps1' -Mode Static
    }
    Invoke-Checked -Id 'rustfmt' -Label 'rustfmt' {
        cargo fmt -- --check
    }
    $identityWatch = [Diagnostics.Stopwatch]::StartNew()
    Import-AgenTermDevelopmentBuildIdentity
    $identityWatch.Stop()
    Write-Host (
        "Development build identity ($($identityWatch.ElapsedMilliseconds) ms)"
    )
    Invoke-Checked -Id 'clippy' -Label 'Clippy' {
        cargo clippy --quiet --locked --all-targets --all-features -- -D warnings
    }
    Invoke-Checked -Id 'unit-tests' -Label 'unit tests' {
        # These real-repository qualification fixtures are intentionally
        # exercised exactly once by their named gates. Running them inside the
        # broad parallel Cargo invocation competes for the same cold CI CPU,
        # process deadlines, Git fixtures, and metadata cache. Release still
        # runs the five-sample benchmark through its named gate.
        cargo test --quiet --locked --all-features -- `
            --skip preflight_task_is_fail_closed_and_writes_reports_for_real_git_fixtures `
            --skip preflight_benchmark_task_measures_clean_public_worker_runs `
            --skip prd_alignment_task_matches_public_catalogs_and_fails_closed `
            --skip supply_chain_task_is_deterministic_and_covers_the_resolved_lock_graph `
            --skip rhai_working_context_smoke_is_private_ephemeral_and_orphan_free `
            --skip rhai_server_smoke_preserves_headless_authority_and_cleanup
    }

    $upgradeGuiFixture = Join-Path (
        [IO.Path]::GetTempPath()
    ) "agenterm-upgrade-gui-$PID-$([Guid]::NewGuid().ToString('N')).exe"
    Invoke-Checked -Id 'artifact-build' `
        -Label 'current artifact and alternate GUI fixture' {
        if (-not $SkipSmoke) {
            & '.\build.bat' release-fast
            if ($LASTEXITCODE -ne 0) {
                throw 'release-fast GUI fixture build failed'
            }
            Copy-Item -LiteralPath '.\dist\agenterm.exe' `
                -Destination $upgradeGuiFixture -Force
        }
        if ($Release) {
            & '.\build.bat' release
        }
        else {
            & '.\build.bat'
        }
    }
    Invoke-Checked -Id 'prd-alignment' -Label 'PRD capability alignment' {
        & '.\dist\agenterm-script.exe' task run prd-alignment `
            --manifest '.\agenterm.tasks.json' `
            --timeout-ms 10000 --max-operations 10000000 -- '.'
    }
    Invoke-Checked -Id 'rhai-lint' -Label 'production Rhai source lint' {
        & '.\lint.ps1' -Mode Rhai `
            -WorkerPath '.\dist\agenterm-script.exe'
    }
    Invoke-Checked -Id 'task-catalog' `
        -Label 'repository Rhai task catalog' {
        & '.\dist\agenterm-script.exe' task check `
            --manifest '.\agenterm.tasks.json'
    }
    # Rhai suites are executable declarations, so discovery must follow the
    # artifact build on a clean checkout while remaining ahead of every smoke.
    $declarationWatch = [Diagnostics.Stopwatch]::StartNew()
    Assert-AgenTermQualificationDeclarations -Context $qualification `
        -SuiteScripts @{
            'cli-smoke' = '.\tests\cli_smoke.ps1'
            'wake-smoke' = '.\scripts\rhai\wake-smoke.rhai'
            'server-smoke' = '.\scripts\rhai\server-smoke.rhai'
            'remote-ui-smoke' = '.\tests\remote_ui_smoke.ps1'
            'remote-ui-upgrade-smoke' = '.\scripts\rhai\remote-ui-upgrade-smoke.rhai'
            'fleet-smoke' = '.\tests\fleet_smoke.ps1'
            'script-smoke' = '.\tests\script_smoke.ps1'
            'theme-smoke' = '.\tests\theme_smoke.ps1'
            'working-context-smoke' = '.\scripts\rhai\working-context-smoke.rhai'
            'workbench-smoke' = '.\tests\workbench_smoke.ps1'
            'ux-smoke' = '.\tests\ux_smoke.ps1'
        }
    $declarationWatch.Stop()
    Write-Host (
        "Qualification declaration discovery " +
        "($($declarationWatch.ElapsedMilliseconds) ms)"
    )
    Invoke-Checked -Id 'migration-audit' `
        -Label 'PowerShell migration ledger and no-new-PS1 gate' {
        & '.\dist\agenterm-script.exe' task run migration-audit `
            --manifest '.\agenterm.tasks.json' `
            --timeout-ms 10000 --max-operations 10000000
    }

    Invoke-Checked -Id 'artifact-verification' `
        -Label 'binary roles and metadata' {
        $cli = '.\dist\agenterm-cli.exe'
        $mux = '.\dist\agenterm-mux.exe'
        $script = '.\dist\agenterm-script.exe'
        $mcp = '.\dist\agenterm-mcp.exe'
        $artifactManifestPath = '.\scripts\artifacts.json'
        & $script task run validate-artifact-manifest `
            --manifest '.\agenterm.tasks.json' -- $artifactManifestPath
        if ($LASTEXITCODE -ne 0) {
            throw (
                'Rhai artifact-manifest validation failed with exit code ' +
                $LASTEXITCODE
            )
        }
        $artifactSpec = Get-Content -LiteralPath $artifactManifestPath -Raw |
            ConvertFrom-Json
        $obsoleteCliArtifacts = @(
            Get-ChildItem -LiteralPath '.\dist' -File -Filter 'agentermctl*.exe'
        )
        if ($obsoleteCliArtifacts.Count -gt 0) {
            throw "dist contains obsolete agentermctl artifacts: $(
                $obsoleteCliArtifacts.Name -join ', '
            )"
        }
        foreach ($artifact in @($artifactSpec.executables)) {
            $path = Join-Path '.\dist' $artifact.name
            $actualSubsystem = Get-PeSubsystem $path
            if ($actualSubsystem -ne [int]$artifact.pe_subsystem) {
                throw (
                    "$($artifact.name) PE subsystem is $actualSubsystem; " +
                    "manifest requires $($artifact.pe_subsystem)."
                )
            }
            if ($Release) {
                $size = (Get-Item -LiteralPath $path).Length
                $budget = [uint64]$artifact.release_budget_bytes
                if ($size -gt $budget) {
                    throw (
                        "Release $($artifact.name) is $size bytes; " +
                        "budget is $budget bytes."
                    )
                }
            }
        }

        $metadata = Get-Content '.\dist\agenterm.json' -Raw | ConvertFrom-Json
        $names = @($metadata.executables.name)
        $expectedNames = @($artifactSpec.executables.name)
        if ($metadata.schema_version -ne 2 -or
            (Compare-Object $expectedNames $names) -or
            $metadata.features -notcontains 'codex-launcher' -or
            $metadata.features -notcontains 'tab-environment' -or
            $metadata.features -notcontains 'mux-frontend') {
            throw 'agenterm.json does not describe all versioned executables.'
        }
        $head = (& git rev-parse HEAD).Trim()
        if ($LASTEXITCODE -ne 0 -or $metadata.git_commit -ne $head) {
            throw 'agenterm.json Git commit does not match the checked source.'
        }
        $cargoLockHash = (
            Get-FileHash '.\Cargo.lock' -Algorithm SHA256
        ).Hash.ToLowerInvariant()
        $artifactManifestHash = (
            Get-FileHash $artifactManifestPath -Algorithm SHA256
        ).Hash.ToLowerInvariant()
        if ($metadata.cargo_lock_sha256 -ne $cargoLockHash -or
            $metadata.artifact_manifest_sha256 -ne $artifactManifestHash -or
            [string]::IsNullOrWhiteSpace([string]$metadata.rust_version)) {
            throw 'agenterm.json provenance does not match locked build inputs.'
        }
        if ($Release -and $metadata.git_dirty) {
            throw 'Release metadata must describe a clean source tree.'
        }
        $scriptVersionOutput = & $script --version
        if ($LASTEXITCODE -ne 0 -or
            $scriptVersionOutput -ne "agenterm-script $($metadata.version)") {
            throw 'agenterm-script --version does not match agenterm.json.'
        }
        $mcpVersionOutput = & $mcp --version
        if ($LASTEXITCODE -ne 0 -or
            $mcpVersionOutput -ne "agenterm-mcp $($metadata.version)") {
            throw 'agenterm-mcp --version does not match agenterm.json.'
        }
        $mcpCapabilities = & $mcp capabilities --json | ConvertFrom-Json
        if ($LASTEXITCODE -ne 0 -or
            $mcpCapabilities.protocol_revision -ne '2025-11-25' -or
            @($mcpCapabilities.transports).Count -ne 1 -or
            $mcpCapabilities.transports[0] -ne 'stdio' -or
            @($mcpCapabilities.resources).Count -ne 4 -or
            @($mcpCapabilities.tools).Count -ne 1 -or
            $mcpCapabilities.tools[0].name -ne 'agenterm_wait' -or
            -not $mcpCapabilities.tools[0].read_only) {
            throw 'agenterm-mcp offline capability catalog is invalid.'
        }

        $muxVersionOutput = & $mux --version
        if ($LASTEXITCODE -ne 0 -or
            $muxVersionOutput -ne "agenterm-mux $($metadata.version) (AgenTerm compatibility frontend)") {
            throw 'agenterm-mux --version does not match agenterm.json.'
        }

        $versionOutput = & $cli --version
        if ($LASTEXITCODE -ne 0 -or
            $versionOutput -ne "agenterm-cli $($metadata.version)") {
            throw 'agenterm-cli --version does not match agenterm.json.'
        }
    }

    Invoke-Checked -Id 'readme-examples' `
        -Label 'manifest-driven README examples' {
        & '.\dist\agenterm-script.exe' task run readme-examples `
            --manifest '.\agenterm.tasks.json' `
            --timeout-ms 10000 --max-operations 10000000
    }
    Invoke-Checked -Id 'docs-site' -Label 'case-exact static docs assets' {
        & '.\dist\agenterm-cli.exe' script run `
            '.\scripts\rhai\verify-docs-site.rhai' `
            --profile local -- $PSScriptRoot\docs
    }
    Invoke-Checked -Id 'supply-chain' `
        -Label 'locked dependency licenses and SPDX inventory' {
        & '.\dist\agenterm-script.exe' task run supply-chain `
            --manifest '.\agenterm.tasks.json' `
            --timeout-ms 60000 --max-operations 10000000 `
            --max-collection-items 100000 --max-string-bytes 8388608 `
            --max-output-bytes 1048576 `
            -- '.' 'dist\agenterm-sbom.spdx.json'
    }
    Invoke-Checked -Id 'target-report' -Label 'Cargo target inventory' {
        & '.\dist\agenterm-cli.exe' script run `
            '.\scripts\rhai\target-report.rhai' `
            --profile local --timeout-ms 10000 --max-operations 10000000 `
            -- $PSScriptRoot target
    }
    Invoke-Checked -Id 'internal-version-policy' `
        -Label 'internal version publication policy' {
        & '.\dist\agenterm-cli.exe' script run `
            '.\scripts\rhai\internal-version-policy.rhai' `
            --profile local --timeout-ms 10000 `
            -- $PSScriptRoot
    }
    Invoke-Checked -Id 'qualification-selftest' `
        -Label 'qualification fail-closed self-test' {
        & '.\scripts\qualification-selftest.ps1'
    }
    Invoke-Checked -Id 'package-boundary-selftest' `
        -Label 'qualified package boundary self-test' {
        & '.\scripts\package-qualified-selftest.ps1'
    }
    Invoke-Checked -Id 'harness-cleanup-selftest' `
        -Label 'owned-resource cleanup self-test' {
        & '.\dist\agenterm-script.exe' task run harness-cleanup-selftest `
            --manifest '.\agenterm.tasks.json' `
            --timeout-ms 10000 --max-operations 10000000
    }
    if (-not $SkipSmoke) {
        # GUI tests must never interrupt the interactive desktop running them.
        # The GUI entry point and CLI autostart both honor this inherited flag.
        $env:AGENTERM_NO_ACTIVATE = '1'
        try {
            Invoke-Checked -Id 'startup-smoke' -Label 'startup smoke test' {
                & '.\tests\startup_smoke.ps1'
            }
            Invoke-Checked -Id 'wake-smoke' `
                -Label 'coalesced runtime wake smoke test' {
                & '.\dist\agenterm-script.exe' task run wake-smoke `
                    --manifest '.\agenterm.tasks.json' `
                    --timeout-ms 60000 --max-operations 10000000
            }
            Invoke-Checked -Id 'cli-smoke' -Label 'CLI smoke test' {
                & '.\tests\cli_smoke.ps1'
            }
            Invoke-Checked -Id 'server-smoke' `
                -Label 'headless server authority smoke test' {
                & '.\dist\agenterm-script.exe' task run server-smoke `
                    --manifest '.\agenterm.tasks.json' `
                    --timeout-ms 60000 --max-operations 10000000
            }
            Invoke-Checked -Id 'remote-ui-smoke' `
                -Label 'replaceable UI client smoke test' {
                & '.\tests\remote_ui_smoke.ps1'
            }
            Invoke-Checked -Id 'remote-ui-upgrade-smoke' `
                -Label 'same-server GUI upgrade and rollback smoke test' {
                & '.\dist\agenterm-script.exe' task run `
                    remote-ui-upgrade-smoke `
                    --manifest '.\agenterm.tasks.json' `
                    --timeout-ms 60000 --max-operations 10000000 -- `
                    $upgradeGuiFixture
            }
            Invoke-Checked -Id 'fleet-smoke' -Label 'AI fleet smoke test' {
                if (-not $IncludeStress) {
                    & '.\tests\fleet_smoke.ps1' -SkipEventLoad
                }
                else {
                    & '.\tests\fleet_smoke.ps1'
                }
            }
            Invoke-Checked -Id 'script-smoke' `
                -Label 'safe scripting smoke test' {
                & '.\tests\script_smoke.ps1'
            }
            Invoke-Checked -Id 'theme-smoke' `
                -Label 'theme settings smoke test' {
                & '.\tests\theme_smoke.ps1'
            }
            Invoke-Checked -Id 'working-context-smoke' `
                -Label 'working context privacy smoke test' {
                & '.\dist\agenterm-script.exe' task run working-context-smoke `
                    --manifest '.\agenterm.tasks.json' `
                    --timeout-ms 60000 --max-operations 10000000
            }
            Invoke-Checked -Id 'workbench-smoke' `
                -Label 'workbench interaction smoke test' {
                & '.\tests\workbench_smoke.ps1'
            }
            Invoke-Checked -Id 'ux-smoke' -Label 'semantic UX smoke test' {
                & '.\tests\ux_smoke.ps1'
            }
        }
        finally {
            if ($hadNoActivateEnvironment) {
                $env:AGENTERM_NO_ACTIVATE = $previousNoActivateEnvironment
            }
            else {
                Remove-Item Env:AGENTERM_NO_ACTIVATE -ErrorAction SilentlyContinue
            }
        }
    }

    # Keep the one-second first-window measurement ahead of the deliberately
    # failure-heavy diagnostic probes. Those probes create and tear down
    # several GUI/worker processes and can transiently distort process-launch
    # latency without exercising the startup path itself.
    Invoke-Checked -Id 'diagnostic-bundle-selftest' `
        -Label 'CLI GUI and script diagnostic bundle self-test' {
        & '.\tests\diagnostic_bundle_selftest.ps1'
    }

    if ($SkipSmoke) {
        Write-Host (
            "`nQUALIFICATION RECEIPT NOT WRITTEN: smoke gates were skipped."
        )
    }
    elseif (-not $IncludeStress) {
        Write-Host (
            "`nQUALIFICATION RECEIPT NOT WRITTEN: explicit stress gate was not included."
        )
    }
    else {
        $writtenReceipt = Write-AgenTermQualificationReceipt `
            -Context $qualification -RepoRoot $PSScriptRoot `
            -OutputPath $receiptPath
        Write-Host "`nQUALIFICATION RECEIPT $writtenReceipt"
    }
    $fullCheckWatch.Stop()
    Write-Host "`nSlowest completed gates:"
    foreach ($result in @(
            $qualification.Results.Values |
                Sort-Object -Property duration_ms -Descending |
                Select-Object -First 8
        )) {
        Write-Host (
            "  $($result.id): $($result.duration_ms) ms"
        )
    }
    Write-Host (
        "Declaration discovery: $($declarationWatch.ElapsedMilliseconds) ms"
    )
    Write-Host "Total quality gate: $($fullCheckWatch.ElapsedMilliseconds) ms"
    Write-Host "`nPASS: AgenTerm quality gate"
}
finally {
    if ($null -ne $upgradeGuiFixture -and
        (Test-Path -LiteralPath $upgradeGuiFixture)) {
        Remove-Item -LiteralPath $upgradeGuiFixture -Force
    }
    Pop-Location
}
