<#
.SYNOPSIS
    Reports which of an executable's static imports the *local* Windows does
    not provide.

.DESCRIPTION
    A static import is resolved by the PE loader before `main` runs, so a
    single missing export refuses the whole program with an entry-point
    dialog naming one symbol. That tells you nothing about the others: fix
    the named one and the loader simply reports the next, one round trip at
    a time.

    Two rounds of that is what prompted this script. `agenterm-con.exe` was
    missing ConPTY on Windows Server 2016, and once that was resolved
    dynamically the loader moved on to `SetThreadDescription` — an export
    Microsoft documents as available in 1607, which *is* Server 2016, but
    which 1607 only implements in KernelBase.dll. Documented minimum
    versions are evidence; the target system is the only proof.

    This walks the PE import table itself, then asks the running system for
    every symbol, and prints the complete missing set in one pass.

    Read-only: it loads libraries and resolves addresses, and calls nothing.

.PARAMETER Path
    The executable or DLL to inspect. Defaults to `agenterm-con.exe` beside
    this script or in a sibling `dist` directory.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File probe-imports.ps1 -Path .\agenterm-con.exe
#>
[CmdletBinding()]
param(
    [string] $Path
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Resolve-Target {
    param([string] $Requested)

    if ($Requested) {
        if (-not (Test-Path -LiteralPath $Requested)) {
            throw "No such file: $Requested"
        }
        return (Resolve-Path -LiteralPath $Requested).Path
    }
    $here = Split-Path -Parent $PSCommandPath
    foreach ($candidate in @(
            (Join-Path $here 'agenterm-con.exe'),
            (Join-Path $here '..\dist\agenterm-con.exe'))) {
        if (Test-Path -LiteralPath $candidate) {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }
    throw 'agenterm-con.exe not found; pass -Path explicitly.'
}

# --- PE import table -------------------------------------------------------
# Parsed by hand rather than with dumpbin, because the machine that needs
# this answer is the target system, which has no Visual Studio on it.

function Get-Imports {
    param([string] $File)

    $bytes = [System.IO.File]::ReadAllBytes($File)
    if ($bytes.Length -lt 0x40 -or $bytes[0] -ne 0x4D -or $bytes[1] -ne 0x5A) {
        throw "$File is not a PE image (no MZ signature)."
    }
    $peOffset = [BitConverter]::ToInt32($bytes, 0x3C)
    if ([BitConverter]::ToUInt32($bytes, $peOffset) -ne 0x00004550) {
        throw "$File is not a PE image (no PE signature)."
    }

    $coff = $peOffset + 4
    $sectionCount = [BitConverter]::ToUInt16($bytes, $coff + 2)
    $optionalSize = [BitConverter]::ToUInt16($bytes, $coff + 16)
    $optional = $coff + 20
    $magic = [BitConverter]::ToUInt16($bytes, $optional)
    # PE32+ moves the data directories 16 bytes later than PE32.
    $directories = if ($magic -eq 0x20B) { $optional + 112 } else { $optional + 96 }
    $importRva = [BitConverter]::ToUInt32($bytes, $directories + 8)
    if ($importRva -eq 0) { return @() }

    # Section headers, to translate RVAs into file offsets.
    $sections = @()
    $sectionBase = $optional + $optionalSize
    for ($i = 0; $i -lt $sectionCount; $i++) {
        $header = $sectionBase + ($i * 40)
        $sections += [pscustomobject]@{
            VirtualAddress = [BitConverter]::ToUInt32($bytes, $header + 12)
            VirtualSize    = [BitConverter]::ToUInt32($bytes, $header + 8)
            RawOffset      = [BitConverter]::ToUInt32($bytes, $header + 20)
            RawSize        = [BitConverter]::ToUInt32($bytes, $header + 16)
        }
    }

    function Convert-Rva {
        param([uint32] $Rva)
        foreach ($section in $sections) {
            $span = [Math]::Max($section.VirtualSize, $section.RawSize)
            if ($Rva -ge $section.VirtualAddress -and $Rva -lt ($section.VirtualAddress + $span)) {
                return [int]($Rva - $section.VirtualAddress + $section.RawOffset)
            }
        }
        return -1
    }

    function Read-Ascii {
        param([int] $Offset)
        $end = $Offset
        while ($end -lt $bytes.Length -and $bytes[$end] -ne 0) { $end++ }
        return [System.Text.Encoding]::ASCII.GetString($bytes, $Offset, $end - $Offset)
    }

    $results = @()
    $descriptor = Convert-Rva $importRva
    while ($true) {
        $nameRva = [BitConverter]::ToUInt32($bytes, $descriptor + 12)
        $thunkRva = [BitConverter]::ToUInt32($bytes, $descriptor)      # OriginalFirstThunk
        if ($thunkRva -eq 0) {
            $thunkRva = [BitConverter]::ToUInt32($bytes, $descriptor + 16)  # FirstThunk
        }
        if ($nameRva -eq 0 -and $thunkRva -eq 0) { break }

        $module = Read-Ascii (Convert-Rva $nameRva)
        $thunk = Convert-Rva $thunkRva
        $stride = if ($magic -eq 0x20B) { 8 } else { 4 }
        while ($true) {
            $entry = if ($magic -eq 0x20B) {
                [BitConverter]::ToUInt64($bytes, $thunk)
            }
            else {
                [uint64][BitConverter]::ToUInt32($bytes, $thunk)
            }
            if ($entry -eq 0) { break }
            # The high bit marks an ordinal import, which carries no name.
            # Shifted rather than written as a literal: PowerShell parses
            # 0x8000000000000000 as a signed Int64 and overflows the cast.
            $ordinalFlag = if ($magic -eq 0x20B) { [uint64]1 -shl 63 } else { [uint64]1 -shl 31 }
            if (($entry -band $ordinalFlag) -eq 0) {
                # The hint/name entry is a 2-byte hint followed by the name.
                $nameEntryRva = [uint32]($entry -band 0x7FFFFFFF)
                $results += [pscustomobject]@{
                    Module = $module
                    Symbol = Read-Ascii ((Convert-Rva $nameEntryRva) + 2)
                }
            }
            $thunk += $stride
        }
        $descriptor += 20
    }
    return $results
}

# --- Live resolution -------------------------------------------------------

if (-not ('AgenTermProbe.Native' -as [type])) {
    Add-Type -Namespace AgenTermProbe -Name Native -MemberDefinition @'
[System.Runtime.InteropServices.DllImport("kernel32", SetLastError = true, CharSet = System.Runtime.InteropServices.CharSet.Unicode)]
public static extern System.IntPtr LoadLibraryW(string name);

[System.Runtime.InteropServices.DllImport("kernel32", SetLastError = true, CharSet = System.Runtime.InteropServices.CharSet.Ansi)]
public static extern System.IntPtr GetProcAddress(System.IntPtr module, string name);
'@
}

$target = Resolve-Target -Requested $Path
Write-Host "Target : $target"
Write-Host "Windows: $([System.Environment]::OSVersion.Version)"
Write-Host ''

$imports = Get-Imports -File $target
if ($imports.Count -eq 0) {
    throw 'No named imports parsed; the probe would report a false all-clear.'
}

# An all-clear is also what a broken probe prints. Before trusting one,
# prove both halves can still fail: a name that cannot exist must not
# resolve, and a real one must.
$selfTest = [AgenTermProbe.Native]::LoadLibraryW('kernel32.dll')
if ($selfTest -eq [System.IntPtr]::Zero) {
    throw 'Cannot load kernel32.dll; the probe cannot trust its own results.'
}
if ([AgenTermProbe.Native]::GetProcAddress($selfTest, 'AgenTermNoSuchExport') -ne [System.IntPtr]::Zero) {
    throw 'A nonexistent export resolved; the probe would report a false all-clear.'
}
if ([AgenTermProbe.Native]::GetProcAddress($selfTest, 'CloseHandle') -eq [System.IntPtr]::Zero) {
    throw 'A universal export failed to resolve; the probe would report false failures.'
}

$missingModules = @()
$missingSymbols = @()
$moduleHandles = @{}

foreach ($group in $imports | Group-Object Module) {
    $module = $group.Name
    $key = $module.ToLowerInvariant()
    if (-not $moduleHandles.ContainsKey($key)) {
        $moduleHandles[$key] = [AgenTermProbe.Native]::LoadLibraryW($module)
    }
    $handle = $moduleHandles[$key]
    if ($handle -eq [System.IntPtr]::Zero) {
        $missingModules += $module
        continue
    }
    foreach ($import in $group.Group) {
        if ([AgenTermProbe.Native]::GetProcAddress($handle, $import.Symbol) -eq [System.IntPtr]::Zero) {
            $missingSymbols += "$module!$($import.Symbol)"
        }
    }
}

Write-Host ("Checked {0} named imports across {1} modules." -f $imports.Count, ($imports | Group-Object Module).Count)
Write-Host ''

if ($missingModules.Count -eq 0 -and $missingSymbols.Count -eq 0) {
    Write-Host 'OK: this system provides every static import. If the program' -ForegroundColor Green
    Write-Host 'still fails to start, the cause is not a missing import.' -ForegroundColor Green
    exit 0
}

if ($missingModules.Count -gt 0) {
    Write-Host 'MISSING MODULES (the loader cannot find these at all):' -ForegroundColor Red
    $missingModules | Sort-Object -Unique | ForEach-Object { Write-Host "  $_" }
    Write-Host ''
}
if ($missingSymbols.Count -gt 0) {
    Write-Host 'MISSING EXPORTS (present module, absent function):' -ForegroundColor Red
    $missingSymbols | Sort-Object -Unique | ForEach-Object { Write-Host "  $_" }
    Write-Host ''
}
Write-Host 'Each line above stops the program before `main`. Send this whole'
Write-Host 'list back rather than the first one: the loader only ever names one.'
exit 1
