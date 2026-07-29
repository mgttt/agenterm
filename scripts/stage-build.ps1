param(
    [Parameter(Mandatory = $true)]
    [string]$SourceDirectory,

    [Parameter(Mandatory = $true)]
    [string]$DestinationDirectory,

    [Parameter(Mandatory = $true)]
    [ValidateSet('dev', 'release-fast', 'release')]
    [string]$Profile
)

$ErrorActionPreference = 'Stop'

$sourceDirectoryPath = [IO.Path]::GetFullPath($SourceDirectory)
$destinationDirectoryPath = [IO.Path]::GetFullPath($DestinationDirectory)
[IO.Directory]::CreateDirectory($destinationDirectoryPath) | Out-Null
$artifactManifestPath = Join-Path $PSScriptRoot 'artifacts.json'
$taskManifestPath = Join-Path (Split-Path -Parent $PSScriptRoot) `
    'agenterm.tasks.json'
$scriptExecutable = Join-Path $sourceDirectoryPath 'agenterm-script.exe'
& $scriptExecutable task run validate-artifact-manifest `
    --manifest $taskManifestPath -- $artifactManifestPath
if ($LASTEXITCODE -ne 0) {
    throw "Rhai artifact-manifest validation failed with exit code $LASTEXITCODE"
}
$artifactManifest = Get-Content -LiteralPath $artifactManifestPath -Raw |
    ConvertFrom-Json

& $scriptExecutable task run clean-locked-artifacts `
    --manifest $taskManifestPath -- `
    $destinationDirectoryPath $artifactManifestPath 'agentermctl.exe'
if ($LASTEXITCODE -ne 0) {
    throw "Rhai locked-artifact cleanup failed with exit code $LASTEXITCODE"
}

foreach ($artifact in @($artifactManifest.executables)) {
    & $scriptExecutable task run stage-artifact `
        --manifest $taskManifestPath -- `
        $sourceDirectoryPath $destinationDirectoryPath $artifact.name
    if ($LASTEXITCODE -ne 0) {
        throw (
            "Rhai staging failed for '$($artifact.name)' with exit code " +
            $LASTEXITCODE
        )
    }
}

& (Join-Path $PSScriptRoot 'write-build-metadata.ps1') `
    -ManifestPath (Join-Path $destinationDirectoryPath 'agenterm.json') `
    -ArtifactManifestPath $artifactManifestPath `
    -StagedDirectory $destinationDirectoryPath `
    -Profile $Profile

& $scriptExecutable task run clean-locked-artifacts `
    --manifest $taskManifestPath -- `
    $destinationDirectoryPath $artifactManifestPath 'agentermctl.exe'
if ($LASTEXITCODE -ne 0) {
    throw "Rhai locked-artifact cleanup failed with exit code $LASTEXITCODE"
}
