param()

$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot

function Invoke-Git {
    param([Parameter(Mandatory = $true)][string[]]$GitArgs)
    & git @GitArgs
    if ($LASTEXITCODE -ne 0) {
        throw "git $($GitArgs -join ' ') failed with exit code $LASTEXITCODE"
    }
}

try {
    $branch = (& git branch --show-current).Trim()
    if ($LASTEXITCODE -ne 0 -or $branch -ne 'main') {
        throw 'Releases must be created from the local main branch.'
    }

    $status = & git status --porcelain
    if ($LASTEXITCODE -ne 0 -or $status) {
        throw 'Commit or discard all working-tree changes before releasing.'
    }

    $cargoToml = Get-Content -LiteralPath '.\Cargo.toml' -Raw
    $match = [regex]::Match(
        $cargoToml,
        '(?ms)^\[package\].*?^version\s*=\s*"(?<version>[^"]+)"'
    )
    if (-not $match.Success) {
        throw 'Could not read the package version from Cargo.toml.'
    }

    $version = $match.Groups['version'].Value
    if ($version -notmatch '^\d+\.\d+\.\d+([+-][0-9A-Za-z.-]+)?$') {
        throw "Cargo package version is not valid semantic versioning: $version"
    }
    if ($version -eq '0.1.7') {
        throw (
            'AgenTerm 0.1.7 is an internal-only consolidation baseline. ' +
            'Do not create a tag or GitHub Release; use the qualified ' +
            'offline package dry-run instead.'
        )
    }
    $tag = "v$version"

    $existingTag = & git tag --list $tag
    if ($LASTEXITCODE -ne 0) {
        throw 'Could not inspect local Git tags.'
    }
    if ($existingTag) {
        throw "Tag $tag already exists locally."
    }

    Write-Host "Preparing AgenTerm $version from $branch"
    & '.\check.ps1' -Release
    if ($LASTEXITCODE -ne 0) {
        throw "Quality gate failed with exit code $LASTEXITCODE"
    }

    Invoke-Git -GitArgs @('tag', '-a', $tag, '-m', "AgenTerm $version")
    try {
        Invoke-Git -GitArgs @('push', '--atomic', 'origin', 'main', $tag)
    }
    catch {
        & git tag -d $tag | Out-Null
        throw
    }

    Write-Host "Published tag $tag. GitHub Actions will create the release."
}
finally {
    Pop-Location
}
