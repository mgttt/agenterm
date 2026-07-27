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

& (Join-Path $PSScriptRoot 'clean-locked-artifacts.ps1') `
    -Directory $destinationDirectoryPath `
    -ObsoleteName 'agentermctl.exe'

$executables = @(
    'agenterm.exe'
    'agenterm-cli.exe'
    'agenterm-mux.exe'
    'agenterm-script.exe'
)
foreach ($name in $executables) {
    & (Join-Path $PSScriptRoot 'stage-artifact.ps1') `
        -Source (Join-Path $sourceDirectoryPath $name) `
        -Destination (Join-Path $destinationDirectoryPath $name)
}

& (Join-Path $PSScriptRoot 'write-build-metadata.ps1') `
    -ManifestPath (Join-Path $destinationDirectoryPath 'agenterm.json') `
    -ExecutablePath (Join-Path $destinationDirectoryPath 'agenterm.exe') `
    -CliExecutablePath (Join-Path $destinationDirectoryPath 'agenterm-cli.exe') `
    -MuxExecutablePath (Join-Path $destinationDirectoryPath 'agenterm-mux.exe') `
    -ScriptExecutablePath (Join-Path $destinationDirectoryPath 'agenterm-script.exe') `
    -Profile $Profile

& (Join-Path $PSScriptRoot 'clean-locked-artifacts.ps1') `
    -Directory $destinationDirectoryPath `
    -ObsoleteName 'agentermctl.exe'
