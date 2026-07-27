param(
    [Parameter(Mandatory = $true)]
    [string]$Directory,

    [string]$ArtifactManifestPath = (Join-Path $PSScriptRoot 'artifacts.json'),

    [string[]]$ObsoleteName = @()
)

$ErrorActionPreference = 'Stop'

$directoryPath = [IO.Path]::GetFullPath($Directory)
if (-not (Test-Path -LiteralPath $directoryPath -PathType Container)) {
    exit 0
}
. (Join-Path $PSScriptRoot 'artifact-manifest.ps1')
$manifest = Get-AgenTermArtifactManifest -Path $ArtifactManifestPath

$removed = 0
$retained = @()
foreach ($entry in @($manifest.executables)) {
    $stem = [IO.Path]::GetFileNameWithoutExtension([string]$entry.name)
    foreach ($artifact in Get-ChildItem -LiteralPath $directoryPath `
        -File -Filter "$stem.locked-*.exe") {
        try {
            Remove-Item -LiteralPath $artifact.FullName -Force -ErrorAction Stop
            $removed++
        }
        catch [IO.IOException] {
            $retained += $artifact.Name
        }
        catch [UnauthorizedAccessException] {
            $retained += $artifact.Name
        }
    }
}

if ($removed -gt 0) {
    Write-Host "Removed $removed stale locked artifact(s) from $directoryPath"
}
if ($retained.Count -gt 0) {
    Write-Host "Retained $($retained.Count) locked artifact(s) still in use; the next build will retry."
}

foreach ($name in $ObsoleteName) {
    if ([IO.Path]::GetFileName($name) -ne $name) {
        throw "Obsolete artifact names must not contain a path: $name"
    }
    $obsoletePath = Join-Path $directoryPath $name
    if (Test-Path -LiteralPath $obsoletePath) {
        Remove-Item -LiteralPath $obsoletePath -Force
        Write-Host "Removed obsolete artifact $name from $directoryPath"
    }
}
