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

    if (-not $SkipSmoke) {
        Invoke-Checked 'CLI smoke test' { & '.\tests\cli_smoke.ps1' }
        Invoke-Checked 'semantic UX smoke test' { & '.\tests\ux_smoke.ps1' }
    }

    Write-Host "`nPASS: AgenTerm quality gate"
}
finally {
    Pop-Location
}
