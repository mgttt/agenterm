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
        if ((Get-PeSubsystem $gui) -ne 2) {
            throw 'agenterm.exe must use the Windows GUI subsystem.'
        }
        if ((Get-PeSubsystem $cli) -ne 3) {
            throw 'agentermctl.exe must use the Windows Console subsystem.'
        }

        $metadata = Get-Content '.\dist\agenterm.json' -Raw | ConvertFrom-Json
        $names = @($metadata.executables.name)
        if ($metadata.schema_version -ne 2 -or
            $names -notcontains 'agenterm.exe' -or
            $names -notcontains 'agentermctl.exe') {
            throw 'agenterm.json does not describe both versioned executables.'
        }

        $versionOutput = & $cli --version
        if ($LASTEXITCODE -ne 0 -or
            $versionOutput -ne "agentermctl $($metadata.version)") {
            throw 'agentermctl --version does not match agenterm.json.'
        }
    }

    if (-not $SkipSmoke) {
        Invoke-Checked 'startup smoke test' { & '.\tests\startup_smoke.ps1' }
        Invoke-Checked 'CLI smoke test' { & '.\tests\cli_smoke.ps1' }
        Invoke-Checked 'semantic UX smoke test' { & '.\tests\ux_smoke.ps1' }
    }

    Write-Host "`nPASS: AgenTerm quality gate"
}
finally {
    Pop-Location
}
