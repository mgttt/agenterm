# Prune stale rustc incremental-compilation caches.
#
# Cargo garbage-collects old *sessions* inside one crate-unit directory, but it
# never removes the crate-unit directories themselves. Every change to a unit's
# fingerprint mints a fresh <crate>-<hash> directory and orphans the previous
# one forever.
#
# Per-crate retention: keep the few most recently used fingerprints of each
# crate and drop the rest. Age is a secondary sweep for crates that stopped
# being built at all.
#
# Deleting these is safe: the only cost of a wrong guess is that one crate
# unit recompiles from scratch next time.
#
# Never fails the caller -- a cleanup problem must not break a build.

param(
    [int]$Keep = 2,
    [int]$AgeDays = 3
)

$ErrorActionPreference = "Stop"

# Opt out entirely
if ($env:AGENTERM_SKIP_INCREMENTAL_PRUNE) {
    exit 0
}

$repo = git rev-parse --show-toplevel 2>$null
if (-not $repo) { exit 0 }

# Cargo target directories are not only `<repo>\target`: nested workspaces
# (crates\*\target, research evidence runs) have their own, and inside one the
# incremental cache sits at a varying depth -- `debug\incremental` for the host
# target but `<triple>\debug\incremental` for cross builds and
# `size-report\release-fast\incremental` for the profile-specific ones. Walk
# the source tree to find every target root, never descending *into* one (they
# hold millions of files), then probe the three known depths.
function Get-TargetRoots([string]$root) {
    $roots = New-Object System.Collections.Generic.List[string]
    $pending = New-Object System.Collections.Generic.Stack[string]
    $pending.Push($root)
    while ($pending.Count -gt 0) {
        $dir = $pending.Pop()
        $children = Get-ChildItem -LiteralPath $dir -Directory -Force -ErrorAction SilentlyContinue
        foreach ($child in $children) {
            if ($child.Name -eq ".git" -or $child.Name -eq "node_modules") { continue }
            if ($child.Name -like "target*") { $roots.Add($child.FullName); continue }
            $pending.Push($child.FullName)
        }
    }
    return $roots
}

$removed = 0
$targets = @()
foreach ($targetRoot in (Get-TargetRoots $repo)) {
    foreach ($depth in @("*", "*\*", "*\*\*")) {
        $targets += Get-ChildItem -Path (Join-Path $targetRoot "$depth\incremental") `
            -Directory -ErrorAction SilentlyContinue
    }
}
$targets = $targets |
    Where-Object { $_.Name -eq "incremental" -and $_.FullName -notmatch "\\incremental\\" } |
    Sort-Object FullName -Unique
if (-not $targets) {
    Write-Host "Cargo incremental prune: skipped (no incremental directories found)"
    exit 0
}

foreach ($incremental in $targets) {
    $units = Get-ChildItem -LiteralPath $incremental.FullName -Directory -ErrorAction SilentlyContinue
    if (-not $units) { continue }

    # Group units by crate name (strip trailing -<hash>)
    $byCrate = @{}
    foreach ($unit in $units) {
        $crateName = $unit.Name -replace '-[^-]*$', ''
        if (-not $byCrate.ContainsKey($crateName)) {
            $byCrate[$crateName] = @()
        }
        $byCrate[$crateName] += $unit
    }

    # Per-crate: keep newest $Keep, remove the rest
    foreach ($crate in $byCrate.Keys) {
        $sorted = $byCrate[$crate] | Sort-Object LastWriteTime -Descending
        $toRemove = $sorted | Select-Object -Skip $Keep
        foreach ($unit in $toRemove) {
            Remove-Item -LiteralPath $unit.FullName -Recurse -Force -ErrorAction SilentlyContinue
            if ($?) { $removed++ }
        }
    }

    # Age sweep: whole crates nobody builds anymore
    $cutoff = (Get-Date).AddDays(-$AgeDays)
    $old = Get-ChildItem -LiteralPath $incremental.FullName -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.LastWriteTime -lt $cutoff }
    foreach ($unit in $old) {
        Remove-Item -LiteralPath $unit.FullName -Recurse -Force -ErrorAction SilentlyContinue
        if ($?) { $removed++ }
    }
}

# The `incremental` directories themselves are never removed -- only the stale
# `<crate>-<hash>` units inside them -- so report the unit count, which is the
# only number that moves.
if ($removed -gt 0) {
    Write-Host "Cargo incremental prune: removed $removed stale cache unit(s) from $($targets.Count) cache dir(s)"
} else {
    Write-Host "Cargo incremental prune: nothing to remove ($($targets.Count) cache dir(s) scanned)"
}
