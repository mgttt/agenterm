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
. (Join-Path $PSScriptRoot 'artifact-manifest.ps1')
$artifactManifest = Get-AgenTermArtifactManifest -Path $artifactManifestPath

& (Join-Path $PSScriptRoot 'clean-locked-artifacts.ps1') `
    -Directory $destinationDirectoryPath `
    -ObsoleteName 'agentermctl.exe'

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

& (Join-Path $PSScriptRoot 'clean-locked-artifacts.ps1') `
    -Directory $destinationDirectoryPath `
    -ObsoleteName 'agentermctl.exe'
