# Build all Windows/x86_64 artifacts for the dynamic-core experiment.
# Produces release binaries (no PDB, no CRT) + flat payload blobs in ..\out\.
# Native PE builds use the MSVC target; variant-B payload blobs are compiled for
# the ELF/sysv64 target (clean flat extraction) and loaded uniformly by the
# Windows kernel, which bridges sysv64 -> win64 when calling OS functions.
$ErrorActionPreference = "Stop"

$Here = Split-Path -Parent $MyInvocation.MyCommand.Path
$Root = Split-Path -Parent $Here
$Out  = Join-Path $Root "out"
New-Item -ItemType Directory -Force -Path $Out | Out-Null

$Sysroot = (& rustc --print sysroot).Trim()
$HostTriple = (((& rustc -vV) | Select-String '^host: ').ToString() -split ' ')[1]
$Lld = Join-Path $Sysroot "lib\rustlib\$HostTriple\bin\rust-lld.exe"

$WinTarget = "x86_64-pc-windows-msvc"
$ElfTarget = "x86_64-unknown-linux-gnu"
# NOTE: the MSVC target mandates unwind tables, so -C force-unwind-tables=no is
# only applied to the ELF blob builds below (an honest, unavoidable platform cost).
$Common = @(
    "--edition", "2021", "-O",
    "-C", "panic=abort", "-C", "debuginfo=0",
    "-A", "unexpected_cfgs"
)

Write-Host "== rustc: $((& rustc --version))"
Write-Host "== lld:   $Lld"

function Build-Exe($src, $cfg, $name) {
    # Compile + link a freestanding PE (no CRT). entry = mainCRTStartup.
    $a = $Common + @(
        "--cfg", "dc_variant=`"$cfg`"", "--cfg", "dc_os=`"windows`"",
        "--target", $WinTarget,
        "-C", "link-args=/subsystem:console /entry:mainCRTStartup /nodefaultlib /DEBUG:NONE",
        $src, "-o", (Join-Path $Out $name)
    )
    & rustc @a
    if ($LASTEXITCODE -ne 0) { throw "rustc failed for $name" }
}

function Build-Blob($src, $name) {
    # Compile for ELF/sysv64 (clean flat extraction), then link a flat PIC image.
    $obj = Join-Path $Out "tmp.o"
    $a = $Common + @(
        "-C", "force-unwind-tables=no",
        "--cfg", "dc_os=`"windows`"", "-C", "relocation-model=pic",
        "--target", $ElfTarget, "--emit=obj", $src, "-o", $obj
    )
    & rustc @a
    if ($LASTEXITCODE -ne 0) { throw "rustc failed for $name" }
    & $Lld -flavor gnu --oformat binary -T (Join-Path $Here "flat.ld") -o (Join-Path $Out $name) $obj
    if ($LASTEXITCODE -ne 0) { throw "lld failed for $name" }
    Remove-Item $obj -Force
}

Write-Host "== variant A (1 layer) =="
Build-Exe "$Root\pack\variant_a_onelayer\pure_compute.rs"    "a" "A_pure_windows.exe"
Build-Exe "$Root\pack\variant_a_onelayer\read_hash_print.rs" "a" "A_rhp_windows.exe"

Write-Host "== variant B (2 layer) - payload blobs =="
Build-Blob "$Root\pack\variant_b_twolayer\payload_pure.rs" "blob_pure_windows.bin"
Build-Blob "$Root\pack\variant_b_twolayer\payload_rhp.rs"  "blob_rhp_windows.bin"

Write-Host "== variant B (2 layer) - kernel/loader (blob embedded) =="
$env:DC_BLOB = (Join-Path $Out "blob_pure_windows.bin")
Build-Exe "$Root\pack\variant_b_twolayer\kernel.rs" "b" "B_kernel_pure_windows.exe"
$env:DC_BLOB = (Join-Path $Out "blob_rhp_windows.bin")
Build-Exe "$Root\pack\variant_b_twolayer\kernel.rs" "b" "B_kernel_rhp_windows.exe"
Remove-Item Env:\DC_BLOB

Write-Host "== done. sizes: =="
Get-ChildItem "$Out\*windows*" | Select-Object Name, Length | Format-Table -AutoSize
