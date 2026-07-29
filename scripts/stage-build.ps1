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
. (Join-Path $PSScriptRoot 'artifact-manifest.ps1')
$artifactManifest = Get-AgenTermArtifactManifest -Path $artifactManifestPath

& $scriptExecutable task run clean-locked-artifacts `
    --manifest $taskManifestPath -- `
    $destinationDirectoryPath $artifactManifestPath 'agentermctl.exe'
if ($LASTEXITCODE -ne 0) {
    throw "Rhai locked-artifact cleanup failed with exit code $LASTEXITCODE"
}

foreach ($artifact in @($artifactManifest.executables)) {
    & (Join-Path $PSScriptRoot 'stage-artifact.ps1') `
        -Source (Join-Path $sourceDirectoryPath $artifact.name) `
        -Destination (Join-Path $destinationDirectoryPath $artifact.name)
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
