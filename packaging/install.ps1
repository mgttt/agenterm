# install.ps1 -- install libagenterm build artifacts into a standard
# MSVC/vcpkg prefix (milestone 71). Windows counterpart of
# packaging/install.sh, which deliberately refuses Windows (its --system
# auto guard: "refusing to install half a layout") and stays untouched.
# The two scripts each own one side; Unix behavior is unchanged.
#
# Layout (MSVC/vcpkg convention -- copied, not invented):
#
#   <includedir>/agenterm.h          default <prefix>\include
#   <libdir>/agenterm.lib            default <prefix>\lib   (static library)
#   <libdir>/agenterm.dll.lib        default <prefix>\lib   (DLL import library)
#   <bindir>/agenterm.dll            default <prefix>\bin   (runtime DLL)
#
# The DLL goes to bin\ rather than lib\: Windows resolves DLLs at runtime
# via PATH / the application directory, while the linker only consults lib\
# at link time. That separation is exactly what lets the milestone 71
# negative control (tests/install_consume_windows.rs) prove static vs
# dynamic linkage. .exp (linker byproduct) and .pdb (debug symbols) are
# NOT installed.
#
# No .pc file is produced: generate-pc.sh only accepts --system
# linux|darwin and pkg-config is not the MSVC consumer convention. The
# Windows system-library truth stays in crates/agenterm-abi/README.md and
# tests/common/mod.rs::system_libs (guarded by the pkgconfig_libs.rs
# four-way anti-drift gate); install.ps1 does not extend generate-pc.sh.
#
# No soname / no versioned file name: the Unix libagenterm.so.1 + symlink
# comes from ELF DT_SONAME; PE has no equivalent mechanism, so the plain
# agenterm.dll is installed flat. abi_version! is deliberately NOT parsed
# here and no agenterm-1.dll is invented.
#
# Idempotent: installing twice into the same prefix overwrites in place and
# yields an identical tree. Non-interactive: no Read-Host or any prompt.
#
# Parameter surface mirrors install.sh (--prefix/--libdir/--includedir/
# --artifacts) so the two stay comparable:
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File packaging\install.ps1 `
#       -Prefix C:\opt\libagenterm
#   powershell -NoProfile -ExecutionPolicy Bypass -File packaging\install.ps1 `
#       -Prefix C:\opt\libagenterm -Artifacts target\abi-release

param(
    [string]$Prefix,
    [string]$LibDir,
    [string]$IncludeDir,
    [string]$BinDir,
    [string]$Artifacts
)

$ErrorActionPreference = "Stop"

$RepoRoot = Split-Path -Parent $PSScriptRoot

function die {
    param([string]$Message)
    [Console]::Error.WriteLine("install.ps1: $Message")
    exit 1
}

# ---- required argument ---------------------------------------------------
if ([string]::IsNullOrWhiteSpace($Prefix)) {
    die "-Prefix is required"
}

# ---- defaults (mirror install.sh) ---------------------------------------
if ([string]::IsNullOrWhiteSpace($LibDir)) { $LibDir = Join-Path $Prefix "lib" }
if ([string]::IsNullOrWhiteSpace($IncludeDir)) { $IncludeDir = Join-Path $Prefix "include" }
if ([string]::IsNullOrWhiteSpace($BinDir)) { $BinDir = Join-Path $Prefix "bin" }
if ([string]::IsNullOrWhiteSpace($Artifacts)) { $Artifacts = Join-Path $RepoRoot "target\abi-release" }

# ---- source artifacts (all must exist before anything is written) --------
if (-not (Test-Path -LiteralPath $Artifacts -PathType Container)) {
    die "artifacts directory not found: $Artifacts (build with --profile abi-release first, or pass -Artifacts)"
}
$StaticLib = Join-Path $Artifacts "agenterm.lib"
$Dll = Join-Path $Artifacts "agenterm.dll"
$ImportLib = Join-Path $Artifacts "agenterm.dll.lib"
$Header = Join-Path $RepoRoot "include\agenterm.h"
if (-not (Test-Path -LiteralPath $StaticLib -PathType Leaf)) { die "static library not found in ${Artifacts}: expected agenterm.lib" }
if (-not (Test-Path -LiteralPath $Dll -PathType Leaf)) { die "dynamic library not found in ${Artifacts}: expected agenterm.dll" }
if (-not (Test-Path -LiteralPath $ImportLib -PathType Leaf)) { die "DLL import library not found in ${Artifacts}: expected agenterm.dll.lib" }
if (-not (Test-Path -LiteralPath $Header -PathType Leaf)) { die "header not found: $Header" }

# ---- install (idempotent: Copy-Item -Force overwrites in place) ----------
New-Item -ItemType Directory -Force -Path $IncludeDir, $LibDir, $BinDir | Out-Null

Copy-Item -LiteralPath $Header -Destination (Join-Path $IncludeDir "agenterm.h") -Force
Copy-Item -LiteralPath $StaticLib -Destination (Join-Path $LibDir "agenterm.lib") -Force
Copy-Item -LiteralPath $ImportLib -Destination (Join-Path $LibDir "agenterm.dll.lib") -Force
Copy-Item -LiteralPath $Dll -Destination (Join-Path $BinDir "agenterm.dll") -Force

# ---- self-check: every promise of the layout must hold -------------------
$Installed = @(
    @{ Path = Join-Path $IncludeDir "agenterm.h";     What = "header" },
    @{ Path = Join-Path $LibDir "agenterm.lib";       What = "static library" },
    @{ Path = Join-Path $LibDir "agenterm.dll.lib";   What = "DLL import library" },
    @{ Path = Join-Path $BinDir "agenterm.dll";       What = "runtime DLL" }
)
foreach ($f in $Installed) {
    if (-not (Test-Path -LiteralPath $f.Path -PathType Leaf)) {
        die "self-check: $($f.What) missing at $($f.Path)"
    }
    $item = Get-Item -LiteralPath $f.Path
    if ($item.Length -le 0) {
        die "self-check: $($f.What) at $($f.Path) is empty"
    }
}

# ---- report (mirror install.sh's tail) -----------------------------------
Write-Output "install.ps1: installed libagenterm (Windows/MSVC layout) into $Prefix"
Write-Output "install.ps1:   include: $(Join-Path $IncludeDir 'agenterm.h')"
Write-Output "install.ps1:   lib:     $(Join-Path $LibDir 'agenterm.lib')"
Write-Output "install.ps1:   lib:     $(Join-Path $LibDir 'agenterm.dll.lib')"
Write-Output "install.ps1:   bin:     $(Join-Path $BinDir 'agenterm.dll')"
