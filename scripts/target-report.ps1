param(
    [string]$RepoRoot = (Join-Path $PSScriptRoot '..'),
    [string]$TargetDirectory,
    [switch]$Json
)

$ErrorActionPreference = 'Stop'
$repoRootPath = [IO.Path]::GetFullPath($RepoRoot)
$repoTargetPath = [IO.Path]::GetFullPath(
    (Join-Path $repoRootPath 'target')
)
if ([string]::IsNullOrWhiteSpace($TargetDirectory)) {
    $TargetDirectory = if (-not [string]::IsNullOrWhiteSpace(
            [string]$env:CARGO_TARGET_DIR
        )) {
        [string]$env:CARGO_TARGET_DIR
    } else {
        $repoTargetPath
    }
}
$targetPath = if ([IO.Path]::IsPathRooted($TargetDirectory)) {
    [IO.Path]::GetFullPath($TargetDirectory)
} else {
    [IO.Path]::GetFullPath((Join-Path $repoRootPath $TargetDirectory))
}
$repoLocal = $targetPath.Equals(
    $repoTargetPath,
    [StringComparison]::OrdinalIgnoreCase
)
$exists = Test-Path -LiteralPath $targetPath -PathType Container
$files = if ($exists) {
    @(Get-ChildItem -LiteralPath $targetPath -File -Recurse -Force)
} else {
    @()
}
$now = [DateTime]::UtcNow
$totalBytes = [uint64]0
foreach ($file in $files) {
    $totalBytes += [uint64]$file.Length
}
$oldest = $files | Sort-Object LastWriteTimeUtc | Select-Object -First 1
$newest = $files | Sort-Object LastWriteTimeUtc -Descending |
    Select-Object -First 1
$profiles = @(
    $files |
        Group-Object {
            $relative = [IO.Path]::GetRelativePath($targetPath, $_.FullName)
            $separator = $relative.IndexOfAny(@(
                    [IO.Path]::DirectorySeparatorChar,
                    [IO.Path]::AltDirectorySeparatorChar
                ))
            if ($separator -lt 0) { '(root)' } else {
                $relative.Substring(0, $separator)
            }
        } |
        Sort-Object Name |
        ForEach-Object {
            $bytes = [uint64]0
            foreach ($file in $_.Group) {
                $bytes += [uint64]$file.Length
            }
            [ordered]@{
                name = $_.Name
                files = $_.Count
                bytes = $bytes
            }
        }
)
$report = [ordered]@{
    schema_version = 1
    target_path = $targetPath
    exists = $exists
    repo_local = $repoLocal
    cleanup_allowed = $repoLocal
    files = $files.Count
    bytes = $totalBytes
    oldest_write_utc = if ($null -eq $oldest) {
        $null
    } else {
        $oldest.LastWriteTimeUtc.ToString('o')
    }
    oldest_age_days = if ($null -eq $oldest) {
        $null
    } else {
        [Math]::Round(($now - $oldest.LastWriteTimeUtc).TotalDays, 3)
    }
    newest_write_utc = if ($null -eq $newest) {
        $null
    } else {
        $newest.LastWriteTimeUtc.ToString('o')
    }
    profiles = @($profiles)
}

if ($Json) {
    $report | ConvertTo-Json -Depth 5
} else {
    Write-Host (
        "Cargo target: $targetPath; repo_local=$repoLocal; " +
        "files=$($files.Count); bytes=$totalBytes; " +
        "oldest_age_days=$($report.oldest_age_days)"
    )
    foreach ($profile in $profiles) {
        Write-Host (
            "  $($profile.name): files=$($profile.files), bytes=$($profile.bytes)"
        )
    }
}
