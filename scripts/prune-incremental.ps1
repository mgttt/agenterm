# Prune stale rustc incremental-compilation caches. Windows counterpart of
# scripts/prune-incremental.sh — see that file for why age alone does not work
# here and why deleting these directories is safe.
#
# Never fails the caller: a cleanup problem must not break a build.

$ErrorActionPreference = 'SilentlyContinue'

if ($env:AGENTERM_SKIP_INCREMENTAL_PRUNE) { exit 0 }

$keep = 2
if ($env:AGENTERM_INCREMENTAL_KEEP) {
    [int]::TryParse($env:AGENTERM_INCREMENTAL_KEEP, [ref]$keep) | Out-Null
}
$ageDays = 3
if ($env:AGENTERM_INCREMENTAL_MAX_AGE_DAYS) {
    [int]::TryParse($env:AGENTERM_INCREMENTAL_MAX_AGE_DAYS, [ref]$ageDays) | Out-Null
}

$repo = git rev-parse --show-toplevel 2>$null
if (-not $repo) { exit 0 }

$cutoff = (Get-Date).AddDays(-$ageDays)
$removed = 0

$roots = Get-ChildItem -Path $repo -Directory -Filter 'target*' |
    ForEach-Object { Get-ChildItem -Path $_.FullName -Directory } |
    ForEach-Object { Join-Path $_.FullName 'incremental' } |
    Where-Object { Test-Path $_ }

foreach ($incremental in $roots) {
    $units = Get-ChildItem -Path $incremental -Directory
    if (-not $units) { continue }

    # Primary rule: keep the most recently used fingerprints per crate. The
    # crate name is the unit directory minus its trailing -<hash>.
    $units |
        Group-Object { $_.Name -replace '-[^-]+$', '' } |
        ForEach-Object {
            $_.Group |
                Sort-Object LastWriteTime -Descending |
                Select-Object -Skip $keep |
                ForEach-Object {
                    Remove-Item -Recurse -Force -LiteralPath $_.FullName
                    if (-not (Test-Path -LiteralPath $_.FullName)) { $script:removed++ }
                }
        }

    # Secondary sweep: crates nobody builds any more still hold up to $keep
    # fingerprints each.
    Get-ChildItem -Path $incremental -Directory |
        Where-Object { $_.LastWriteTime -lt $cutoff } |
        ForEach-Object {
            Remove-Item -Recurse -Force -LiteralPath $_.FullName
            if (-not (Test-Path -LiteralPath $_.FullName)) { $script:removed++ }
        }
}

if ($removed -gt 0) {
    Write-Host "pruned $removed stale incremental cache unit(s)"
}

exit 0
