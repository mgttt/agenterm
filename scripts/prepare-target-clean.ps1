param(
    [string]$RepoRoot = (Join-Path $PSScriptRoot '..')
)

$ErrorActionPreference = 'Stop'
$repo = [IO.Path]::GetFullPath($RepoRoot).TrimEnd(
    [IO.Path]::DirectorySeparatorChar,
    [IO.Path]::AltDirectorySeparatorChar
)
$gitRootLines = @(& git -C $repo rev-parse --show-toplevel 2>&1)
if ($LASTEXITCODE -ne 0 -or $gitRootLines.Count -eq 0) {
    throw "Could not verify the repository root: $($gitRootLines -join ' ')"
}
$gitRoot = [IO.Path]::GetFullPath(
    ([string]$gitRootLines[0]).Trim()
).TrimEnd(
    [IO.Path]::DirectorySeparatorChar,
    [IO.Path]::AltDirectorySeparatorChar
)
if (-not $repo.Equals($gitRoot, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing target preparation outside the exact Git root: $repo"
}

$target = [IO.Path]::GetFullPath((Join-Path $repo 'target'))
$expected = [IO.Path]::GetFullPath("$repo$([IO.Path]::DirectorySeparatorChar)target")
if (-not $target.Equals($expected, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing to prepare an unexpected Cargo target: $target"
}
if (-not (Test-Path -LiteralPath $target -PathType Container)) {
    throw "Cargo target does not exist after the release build: $target"
}
$targetInfo = Get-Item -LiteralPath $target -Force
if (($targetInfo.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
    throw "Refusing to prepare a reparse-point Cargo target: $target"
}

$signature = 'Signature: 8a477f597d28d172789f06886806bc55'
if ($signature -notmatch '^Signature: [0-9a-f]{32}$') {
    throw 'Internal Cargo cache tag signature is malformed'
}
$content = @"
$signature
# This file is a cache directory tag created by cargo.
# For information about cache directory tags see https://bford.info/cachedir/
"@ -replace "`r`n", "`n"
$tag = Join-Path $target 'CACHEDIR.TAG'
if (Test-Path -LiteralPath $tag -PathType Leaf) {
    $existing = [IO.File]::ReadAllText($tag)
    if (-not $existing.StartsWith("$signature`n", [StringComparison]::Ordinal)) {
        throw "Refusing to overwrite an invalid Cargo cache tag: $tag"
    }
} else {
    [IO.File]::WriteAllText(
        $tag,
        $content,
        [Text.UTF8Encoding]::new($false)
    )
}
Write-Host "Prepared Cargo cache tag for exact repo target: $target"
