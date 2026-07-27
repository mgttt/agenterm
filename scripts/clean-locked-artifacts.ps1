param(
    [Parameter(Mandatory = $true)]
    [string]$Directory
)

$ErrorActionPreference = 'Stop'

$directoryPath = [IO.Path]::GetFullPath($Directory)
if (-not (Test-Path -LiteralPath $directoryPath -PathType Container)) {
    exit 0
}

$removed = 0
$retained = @()
foreach ($artifact in Get-ChildItem -LiteralPath $directoryPath -File -Filter '*locked*') {
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

if ($removed -gt 0) {
    Write-Host "Removed $removed stale locked artifact(s) from $directoryPath"
}
if ($retained.Count -gt 0) {
    Write-Host "Retained $($retained.Count) locked artifact(s) still in use; the next build will retry."
}
