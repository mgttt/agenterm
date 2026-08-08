# Build + measure all Q3 (lowering-cost) artifacts on Windows/x86_64.
# Produces: in-kernel variant-A exes (lowerer inside), out-of-kernel variant-B exes
# (minimal Q0 kernel + lowerer blob), a no-lowerer baseline kernel (④ TCB baseline),
# the lowerer-isolation object (① X), and the three IR .bin payloads.
$ErrorActionPreference = "Stop"

$Here = Split-Path -Parent $MyInvocation.MyCommand.Path
$L    = Split-Path -Parent $Here          # ...\lowering
$Root = Split-Path -Parent $L             # ...\dynamic-core
$Out  = Join-Path $L "out"
New-Item -ItemType Directory -Force -Path $Out | Out-Null

$Sysroot = (& rustc --print sysroot).Trim()
$HostTriple = (((& rustc -vV) | Select-String '^host: ').ToString() -split ' ')[1]
$Lld     = Join-Path $Sysroot "lib\rustlib\$HostTriple\bin\rust-lld.exe"
$Objcopy = Join-Path $Sysroot "lib\rustlib\$HostTriple\bin\rust-objcopy.exe"
$WinTarget = "x86_64-pc-windows-msvc"
$ElfTarget = "x86_64-unknown-linux-gnu"
$FlatLd  = Join-Path $Root "build\flat.ld"
$Common  = @("--edition","2021","-O","-C","panic=abort","-C","debuginfo=0","-A","unexpected_cfgs","-A","dead_code")

Write-Host "== rustc: $((& rustc --version))"

function Build-ExeWin($src, $cfgs, $name) {
    $a = $Common + $cfgs + @(
        "--target", $WinTarget,
        "-C", "link-args=/subsystem:console /entry:mainCRTStartup /nodefaultlib /DEBUG:NONE",
        $src, "-o", (Join-Path $Out $name))
    & rustc @a
    if ($LASTEXITCODE -ne 0) { throw "rustc failed: $name" }
}
function Build-BlobWin($src, $cfgs, $name) {
    # -min-jump-table-entries=200 suppresses jump tables: the minimal (non-relocating)
    # loader copies+jumps the flat blob with NO relocations, and a jump table breaks
    # under that. This is a REAL constraint the out-of-kernel packaging imposes on the
    # lowerer (a code generator, unlike Q0's precompiled payloads). See RESULTS §8.
    $obj = Join-Path $Out "tmp.o"
    $a = $Common + $cfgs + @(
        "-C", "force-unwind-tables=no", "-C", "relocation-model=pic",
        "-C", "llvm-args=-min-jump-table-entries=200",
        "--target", $ElfTarget, "--emit=obj", $src, "-o", $obj)
    & rustc @a
    if ($LASTEXITCODE -ne 0) { throw "rustc failed: $name" }
    & $Lld -flavor gnu --oformat binary -T $FlatLd -o (Join-Path $Out $name) $obj
    if ($LASTEXITCODE -ne 0) { throw "lld failed: $name" }
    Remove-Item $obj -Force
}

# 0) IR payloads (author-time; not part of X)
& rustc -O "$L\tools\ir_gen.rs" -o "$Out\ir_gen.exe" | Out-Null
& "$Out\ir_gen.exe" $Out | Out-Null

$payloads = @("pure","rhp","spawn")

Write-Host "== in-kernel (variant A: lowerer statically linked) =="
foreach ($p in $payloads) {
    $env:DC_IR = Join-Path $Out "ir_$p.bin"
    Build-ExeWin "$L\pack\in_kernel.rs" @("--cfg","dc_variant=`"a`"","--cfg","dc_os=`"windows`"") "A_lower_${p}_windows.exe"
    Remove-Item Env:\DC_IR
}

Write-Host "== out-of-kernel (variant B: lowerer as a flat blob) =="
foreach ($p in $payloads) {
    $env:DC_IR = Join-Path $Out "ir_$p.bin"
    Build-BlobWin "$L\pack\out_kernel_blob.rs" @("--cfg","dc_os=`"windows`"") "lowblob_${p}_windows.bin"
    Remove-Item Env:\DC_IR
    $env:DC_BLOB = Join-Path $Out "lowblob_${p}_windows.bin"
    Build-ExeWin "$Root\pack\variant_b_twolayer\kernel.rs" @("--cfg","dc_variant=`"b`"","--cfg","dc_os=`"windows`"") "B_lower_${p}_windows.exe"
    Remove-Item Env:\DC_BLOB
}

Write-Host "== baseline: Q0 minimal kernel with a precompiled NATIVE blob (no lowerer) =="
# This is the ④ TCB baseline: the frozen minimal kernel that CANNOT run IR.
Build-BlobWin "$Root\pack\variant_b_twolayer\payload_pure.rs" @("--cfg","dc_os=`"windows`"") "native_pure_windows.bin"
$env:DC_BLOB = Join-Path $Out "native_pure_windows.bin"
Build-ExeWin "$Root\pack\variant_b_twolayer\kernel.rs" @("--cfg","dc_variant=`"b`"","--cfg","dc_os=`"windows`"") "baseline_kernel_windows.exe"
Remove-Item Env:\DC_BLOB

Write-Host "== isolate X: two flat blobs differing only by the lowerer =="
# X = size(lower flat) - size(driver flat), both flat-PIC binaries (Q0 blob口径, ELF target).
Build-BlobWin "$L\pack\measure_lower_flat.rs"  @() "mx_lower_flat.bin"
Build-BlobWin "$L\pack\measure_driver_flat.rs" @() "mx_driver_flat.bin"
$xl = (Get-Item "$Out\mx_lower_flat.bin").Length
$xd = (Get-Item "$Out\mx_driver_flat.bin").Length
Write-Host ("   lower flat = {0} B ; driver flat = {1} B ; X = {2} B" -f $xl,$xd,($xl-$xd))

Write-Host "== sizes =="
Get-ChildItem "$Out\*windows*.exe","$Out\lowblob_*.bin","$Out\native_pure_windows.bin","$Out\ir_*.bin" |
    Select-Object Name,Length | Sort-Object Name | Format-Table -AutoSize
