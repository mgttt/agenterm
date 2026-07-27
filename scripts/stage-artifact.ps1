param(
    [Parameter(Mandatory = $true)]
    [string]$Source,

    [Parameter(Mandatory = $true)]
    [string]$Destination
)

$ErrorActionPreference = 'Stop'

$sourcePath = (Resolve-Path -LiteralPath $Source).Path
$destinationPath = [IO.Path]::GetFullPath($Destination)
$destinationDirectory = [IO.Path]::GetDirectoryName($destinationPath)
[IO.Directory]::CreateDirectory($destinationDirectory) | Out-Null

try {
    Copy-Item -LiteralPath $sourcePath -Destination $destinationPath -Force
    exit 0
}
catch [IO.IOException] {
    if (-not (Test-Path -LiteralPath $destinationPath)) {
        throw
    }

    $name = [IO.Path]::GetFileNameWithoutExtension($destinationPath)
    $extension = [IO.Path]::GetExtension($destinationPath)
    $suffix = "{0}-{1}" -f $PID, [DateTime]::UtcNow.Ticks
    $parkedPath = Join-Path $destinationDirectory "$name.locked-$suffix$extension"

    Move-Item -LiteralPath $destinationPath -Destination $parkedPath
    try {
        Copy-Item -LiteralPath $sourcePath -Destination $destinationPath
    }
    catch {
        Move-Item -LiteralPath $parkedPath -Destination $destinationPath -ErrorAction SilentlyContinue
        throw
    }

    Write-Host "Staged new artifact; running copy remains at $parkedPath"
}
