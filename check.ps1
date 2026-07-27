param(
    [switch]$Release,
    [switch]$SkipSmoke,
    [switch]$IncludeStress
)

$ErrorActionPreference = 'Stop'
$hadNoActivateEnvironment = Test-Path Env:AGENTERM_NO_ACTIVATE
$previousNoActivateEnvironment = $env:AGENTERM_NO_ACTIVATE
Push-Location $PSScriptRoot

function Invoke-Checked {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Label,

        [Parameter(Mandatory = $true)]
        [scriptblock]$Command
    )

    Write-Host "`n==> $Label"
    & $Command
    if ($LASTEXITCODE -ne 0) {
        throw "$Label failed with exit code $LASTEXITCODE"
    }
}

function Get-PeSubsystem {
    param([Parameter(Mandatory = $true)][string]$Path)

    $bytes = [IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $Path))
    $peOffset = [BitConverter]::ToInt32($bytes, 0x3c)
    [BitConverter]::ToUInt16($bytes, $peOffset + 24 + 68)
}

try {
    Invoke-Checked 'rustfmt' { cargo fmt -- --check }
    Invoke-Checked 'Clippy' {
        cargo clippy --locked --all-targets --all-features -- -D warnings
    }
    Invoke-Checked 'unit tests' { cargo test --locked --all-features }

    if ($Release) {
        Invoke-Checked 'release artifact' { & '.\build.bat' release }
    }
    else {
        Invoke-Checked 'development artifact' { & '.\build.bat' }
    }

    Invoke-Checked 'binary roles and metadata' {
        $gui = '.\dist\agenterm.exe'
        $cli = '.\dist\agenterm-cli.exe'
        $mux = '.\dist\agenterm-mux.exe'
        $script = '.\dist\agenterm-script.exe'
        $artifactManifestPath = '.\scripts\artifacts.json'
        . '.\scripts\artifact-manifest.ps1'
        $artifactSpec = Get-AgenTermArtifactManifest -Path $artifactManifestPath
        $releaseBudgets = [ordered]@{}
        foreach ($artifact in @($artifactSpec.executables)) {
            $releaseBudgets[$artifact.name] = [uint64]$artifact.release_budget_bytes
        }
        $obsoleteCliArtifacts = @(
            Get-ChildItem -LiteralPath '.\dist' -File -Filter 'agentermctl*.exe'
        )
        if ($obsoleteCliArtifacts.Count -gt 0) {
            throw "dist contains obsolete agentermctl artifacts: $(
                $obsoleteCliArtifacts.Name -join ', '
            )"
        }
        if ((Get-PeSubsystem $gui) -ne 2) {
            throw 'agenterm.exe must use the Windows GUI subsystem.'
        }
        if ((Get-PeSubsystem $cli) -ne 3) {
            throw 'agenterm-cli.exe must use the Windows Console subsystem.'
        }
        if ((Get-PeSubsystem $mux) -ne 3) {
            throw 'agenterm-mux.exe must use the Windows Console subsystem.'
        }
        if ((Get-PeSubsystem $script) -ne 3) {
            throw 'agenterm-script.exe must use the Windows Console subsystem.'
        }
        if ($Release) {
            foreach ($entry in $releaseBudgets.GetEnumerator()) {
                $path = Join-Path '.\dist' $entry.Key
                $size = (Get-Item -LiteralPath $path).Length
                if ($size -gt $entry.Value) {
                    throw "Release $($entry.Key) is $size bytes; budget is $($entry.Value) bytes."
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

    Invoke-Checked 'PRD capability alignment' {
        & '.\tests\prd_alignment.ps1'
    }

    if (-not $SkipSmoke) {
        # GUI tests must never interrupt the interactive desktop running them.
        # The GUI entry point and CLI autostart both honor this inherited flag.
        $env:AGENTERM_NO_ACTIVATE = '1'
        try {
            Invoke-Checked 'startup smoke test' { & '.\tests\startup_smoke.ps1' }
            Invoke-Checked 'CLI smoke test' { & '.\tests\cli_smoke.ps1' }
            Invoke-Checked 'AI fleet smoke test' {
                if (-not $IncludeStress) {
                    & '.\tests\fleet_smoke.ps1' -SkipEventLoad
                }
                else {
                    & '.\tests\fleet_smoke.ps1'
                }
            }
            Invoke-Checked 'safe scripting smoke test' { & '.\tests\script_smoke.ps1' }
            Invoke-Checked 'theme settings smoke test' { & '.\tests\theme_smoke.ps1' }
            Invoke-Checked 'working context privacy smoke test' {
                & '.\tests\working_context_smoke.ps1'
            }
            Invoke-Checked 'semantic UX smoke test' { & '.\tests\ux_smoke.ps1' }
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

    Write-Host "`nPASS: AgenTerm quality gate"
}
finally {
    Pop-Location
}
