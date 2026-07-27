param(
    [string]$RepoRoot = (Join-Path $PSScriptRoot '..'),
    [string]$ArtifactDirectory = 'dist',
    [switch]$SkipExecutableProbes
)

$ErrorActionPreference = 'Stop'
$repoRootPath = [IO.Path]::GetFullPath($RepoRoot)
$artifactDirectoryPath = [IO.Path]::GetFullPath(
    (Join-Path $repoRootPath $ArtifactDirectory)
)
. (Join-Path $repoRootPath 'scripts\artifact-manifest.ps1')
$manifest = Get-AgenTermArtifactManifest `
    -Path (Join-Path $repoRootPath 'scripts\artifacts.json')
$readme = Get-Content -LiteralPath (Join-Path $repoRootPath 'README.md') -Raw
$normalizedReadme = [regex]::Replace($readme, '\s+', ' ')

foreach ($artifact in @($manifest.executables)) {
    if ($readme -notmatch [regex]::Escape([string]$artifact.name)) {
        throw "README does not name manifest artifact '$($artifact.name)'."
    }
    $role = [regex]::Replace([string]$artifact.documentation_role, '\s+', ' ')
    if ($normalizedReadme -notmatch [regex]::Escape($role)) {
        throw "README does not describe '$($artifact.name)' as '$role'."
    }
}

foreach ($requiredExample in @(
        '.\build.bat',
        '.\check.ps1',
        '.\dist\agenterm.exe'
    )) {
    if (-not $readme.Contains($requiredExample)) {
        throw "README is missing required executable example '$requiredExample'."
    }
}

if (-not $SkipExecutableProbes) {
    $hadNoActivate = Test-Path Env:AGENTERM_NO_ACTIVATE
    $previousNoActivate = $env:AGENTERM_NO_ACTIVATE
    $env:AGENTERM_NO_ACTIVATE = '1'
    try {
        foreach ($artifact in @($manifest.executables)) {
            $probe = @($artifact.offline_probe)
            if ($probe.Count -eq 0) {
                continue
            }
            $path = Join-Path $artifactDirectoryPath $artifact.name
            if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
                throw "README probe artifact is missing: $path"
            }
            & $path @probe | Out-Null
            if ($LASTEXITCODE -ne 0) {
                throw "Offline README probe failed for '$($artifact.name)'."
            }
        }

        $cli = Join-Path $artifactDirectoryPath 'agenterm-cli.exe'
        $cliCommands = @(& $cli list-commands)
        if ($LASTEXITCODE -ne 0) {
            throw 'Could not read the offline agenterm-cli command catalog.'
        }
        $cliNames = [Collections.Generic.HashSet[string]]::new(
            [StringComparer]::Ordinal
        )
        foreach ($line in $cliCommands) {
            foreach ($name in [regex]::Matches(
                    [string]$line,
                    '[a-z][a-z0-9-]*'
                ).Value) {
                [void]$cliNames.Add($name)
            }
        }
        foreach ($match in [regex]::Matches(
                $readme,
                '(?m)^\s*& \$r (?<arguments>[^\r\n]+)$'
            )) {
            $tokens = @(
                [regex]::Matches(
                    $match.Groups['arguments'].Value,
                    '(?:"[^"]*"|[^\s]+)'
                ).Value
            )
            $commandIndex = if ($tokens.Count -ge 3 -and
                $tokens[0] -eq '--address') { 2 } else { 0 }
            $command = $tokens[$commandIndex].Trim('"')
            if (-not $cliNames.Contains($command)) {
                throw "README references unknown agenterm-cli command '$command'."
            }
        }

        $mux = Join-Path $artifactDirectoryPath 'agenterm-mux.exe'
        $compatibility = & $mux compatibility --json | ConvertFrom-Json
        if ($LASTEXITCODE -ne 0 -or $null -eq $compatibility) {
            throw 'README mux compatibility example failed offline.'
        }
    }
    finally {
        if ($hadNoActivate) {
            $env:AGENTERM_NO_ACTIVATE = $previousNoActivate
        }
        else {
            Remove-Item Env:AGENTERM_NO_ACTIVATE -ErrorAction SilentlyContinue
        }
    }
}

Write-Host (
    "PASS: README names $(@($manifest.executables).Count) manifest artifacts " +
    'and its command examples match the offline catalogs.'
)
