param(
    [switch]$Release,
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

function Get-PeSubsystem {
    param([Parameter(Mandatory = $true)][string]$Path)

    $bytes = [IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $Path))
    $peOffset = [BitConverter]::ToInt32($bytes, 0x3c)
    [BitConverter]::ToUInt16($bytes, $peOffset + 24 + 68)
}

try {
    if ($InternalQualificationDryRun) {
        & '.\scripts\qualification-selftest.ps1'
        return
    }

    $receiptPath = [IO.Path]::GetFullPath($QualificationReceiptPath)
    if (Test-Path -LiteralPath $receiptPath) {
        Remove-Item -LiteralPath $receiptPath -Force
    }
    . '.\scripts\qualification.ps1'
    $qualificationManifestPath = '.\scripts\qualification-gates.json'
    $qualification = New-AgenTermQualificationContext `
        -ManifestPath $qualificationManifestPath `
        -Release ([bool]$Release) `
        -StressIncluded ([bool]$IncludeStress)
    Assert-AgenTermQualificationDeclarations -Context $qualification `
        -SuiteScripts @{
            'cli-smoke' = '.\tests\cli_smoke.ps1'
            'fleet-smoke' = '.\tests\fleet_smoke.ps1'
            'script-smoke' = '.\tests\script_smoke.ps1'
            'theme-smoke' = '.\tests\theme_smoke.ps1'
            'working-context-smoke' = '.\tests\working_context_smoke.ps1'
            'proxy-smoke' = '.\tests\proxy_smoke.ps1'
            'workbench-smoke' = '.\tests\workbench_smoke.ps1'
            'ux-smoke' = '.\tests\ux_smoke.ps1'
        }
    Invoke-Checked -Id 'preflight-selftest' `
        -Label 'read-only preflight self-test' {
        & '.\scripts\preflight-selftest.ps1'
    }
    if ($Release) {
        Invoke-Checked -Id 'release-preflight' `
            -Label 'clean internal candidate preflight' {
            & '.\scripts\preflight.ps1'
        }
        Invoke-Checked -Id 'preflight-benchmark' `
            -Label 'local preflight p95 benchmark' {
            & '.\scripts\preflight-benchmark.ps1' -Iterations 5
        }
    }
    Invoke-Checked -Id 'rustfmt' -Label 'rustfmt' {
        cargo fmt -- --check
    }
    Invoke-Checked -Id 'clippy' -Label 'Clippy' {
        cargo clippy --locked --all-targets --all-features -- -D warnings
    }
    Invoke-Checked -Id 'unit-tests' -Label 'unit tests' {
        cargo test --locked --all-features
    }

    if ($Release) {
        Invoke-Checked -Id 'artifact-build' -Label 'release artifact' {
            & '.\build.bat' release
        }
    }
    else {
        Invoke-Checked -Id 'artifact-build' -Label 'development artifact' {
            & '.\build.bat'
        }
    }

    Invoke-Checked -Id 'artifact-verification' `
        -Label 'binary roles and metadata' {
        $cli = '.\dist\agenterm-cli.exe'
        $mux = '.\dist\agenterm-mux.exe'
        $script = '.\dist\agenterm-script.exe'
        $artifactManifestPath = '.\scripts\artifacts.json'
        . '.\scripts\artifact-manifest.ps1'
        $artifactSpec = Get-AgenTermArtifactManifest -Path $artifactManifestPath
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
        & '.\tests\readme_examples.ps1'
    }
    Invoke-Checked -Id 'supply-chain' `
        -Label 'locked dependency licenses and SPDX inventory' {
        & '.\scripts\supply-chain.ps1'
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
        & '.\tests\harness_cleanup_selftest.ps1'
    }
    Invoke-Checked -Id 'diagnostic-bundle-selftest' `
        -Label 'CLI GUI and script diagnostic bundle self-test' {
        & '.\tests\diagnostic_bundle_selftest.ps1'
    }

    Invoke-Checked -Id 'prd-alignment' -Label 'PRD capability alignment' {
        & '.\tests\prd_alignment.ps1'
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
                & '.\tests\wake_smoke.ps1'
            }
            Invoke-Checked -Id 'cli-smoke' -Label 'CLI smoke test' {
                & '.\tests\cli_smoke.ps1'
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
                & '.\tests\working_context_smoke.ps1'
            }
            Invoke-Checked -Id 'proxy-smoke' `
                -Label 'confirmed proxy application smoke test' {
                & '.\tests\proxy_smoke.ps1'
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
    Write-Host "`nPASS: AgenTerm quality gate"
}
finally {
    Pop-Location
}
