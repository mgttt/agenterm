param(
    [switch]$Release,
    [switch]$SkipSmoke
)

$ErrorActionPreference = 'Stop'
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
        cargo clippy --all-targets --all-features -- -D warnings
    }
    Invoke-Checked 'unit tests' { cargo test --all-features }

    if ($Release) {
        Invoke-Checked 'release artifact' { & '.\build.bat' release }
    }
    else {
        Invoke-Checked 'development artifact' { & '.\build.bat' }
    }

    Invoke-Checked 'binary roles and metadata' {
        $gui = '.\dist\agenterm.exe'
        $cli = '.\dist\agentermctl.exe'
        $mux = '.\dist\agenterm-mux.exe'
        $script = '.\dist\agenterm-script.exe'
        $releaseBudgets = [ordered]@{
            'agenterm.exe'     = 4MB
            'agentermctl.exe'  = 2MB
            'agenterm-mux.exe' = 2MB
            'agenterm-script.exe' = 3MB
        }
        if ((Get-PeSubsystem $gui) -ne 2) {
            throw 'agenterm.exe must use the Windows GUI subsystem.'
        }
        if ((Get-PeSubsystem $cli) -ne 3) {
            throw 'agentermctl.exe must use the Windows Console subsystem.'
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
        if ($metadata.schema_version -ne 2 -or
            $names -notcontains 'agenterm.exe' -or
            $names -notcontains 'agentermctl.exe' -or
            $names -notcontains 'agenterm-mux.exe' -or
            $names -notcontains 'agenterm-script.exe' -or
            $metadata.features -notcontains 'codex-launcher' -or
            $metadata.features -notcontains 'tab-environment' -or
            $metadata.features -notcontains 'mux-frontend') {
            throw 'agenterm.json does not describe all versioned executables.'
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
            $versionOutput -ne "agentermctl $($metadata.version)") {
            throw 'agentermctl --version does not match agenterm.json.'
        }
    }

    Invoke-Checked 'PRD capability alignment' {
        & '.\tests\prd_alignment.ps1'
    }

    if (-not $SkipSmoke) {
        Invoke-Checked 'startup smoke test' { & '.\tests\startup_smoke.ps1' }
        Invoke-Checked 'CLI smoke test' { & '.\tests\cli_smoke.ps1' }
        Invoke-Checked 'AI fleet smoke test' { & '.\tests\fleet_smoke.ps1' }
        Invoke-Checked 'safe scripting smoke test' { & '.\tests\script_smoke.ps1' }
        Invoke-Checked 'semantic UX smoke test' { & '.\tests\ux_smoke.ps1' }
    }

    Write-Host "`nPASS: AgenTerm quality gate"
}
finally {
    Pop-Location
}
