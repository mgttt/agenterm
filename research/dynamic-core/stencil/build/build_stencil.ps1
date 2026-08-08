# Build + measure all Q10 (copy-and-patch stencil) artifacts on Windows/x86_64.
# Produces: the stencil object (rustc -O2) + generated stencil-data table, the three
# in-kernel exes (applier + stencil data + IR), and the flat X_total isolation.
$ErrorActionPreference = "Stop"
$Here = Split-Path -Parent $MyInvocation.MyCommand.Path
$S    = Split-Path -Parent $Here            # ...\stencil
$Root = Split-Path -Parent $S               # ...\dynamic-core
$Out  = Join-Path $S "out"
New-Item -ItemType Directory -Force -Path $Out | Out-Null

$Sysroot    = (& rustc --print sysroot).Trim()
$HostTriple = (((& rustc -vV) | Select-String '^host: ').ToString() -split ' ')[1]
$Lld    = Join-Path $Sysroot "lib\rustlib\$HostTriple\bin\rust-lld.exe"
$Size   = Join-Path $Sysroot "lib\rustlib\$HostTriple\bin\llvm-size.exe"
$FlatLd = Join-Path $Root "build\flat.ld"
$Common = @("--edition","2021","-O","-C","panic=abort","-C","debuginfo=0","-A","unexpected_cfgs","-A","dead_code")
$la = "link-args=/subsystem:console /entry:mainCRTStartup /nodefaultlib /DEBUG:NONE"

Write-Host "== 1) real compiler emits the stencils (rustc -O2 -> ELF obj) =="
& rustc @Common -C relocation-model=static -C force-unwind-tables=no --target x86_64-unknown-linux-gnu --emit=obj "$S\stencils.rs" -o "$Out\stencils.o"
Write-Host "== 2) build tool extracts bytes+holes -> stencils_gen.rs (the STENCIL DATA) =="
& rustc -O "$S\stencilize.rs" -o "$Out\stencilize.exe"
& "$Out\stencilize.exe" "$Out\stencils.o" "$Out\stencils_gen.rs"

Write-Host "== 3) IR payloads (Q2 authoring tool, unchanged semantics) =="
& rustc -O "$Root\lowering\tools\ir_gen.rs" -o "$Out\ir_gen.exe" | Out-Null
& "$Out\ir_gen.exe" $Out | Out-Null

Write-Host "== 4) in-kernel exes (applier + stencil data + IR, statically linked) =="
foreach ($p in @("pure","rhp","spawn")) {
    $env:DC_IR = Join-Path $Out "ir_$p.bin"
    $a = $Common + @("--cfg","dc_variant=`"a`"","--cfg","dc_os=`"windows`"","--target","x86_64-pc-windows-msvc","-C",$la,"$S\pack\in_kernel.rs","-o",(Join-Path $Out "A_st_${p}_windows.exe"))
    & rustc @a
    if ($LASTEXITCODE -ne 0) { throw "rustc in_kernel $p" }
}
$env:DC_IR = "none"

Write-Host "== 5) isolate X_total: two flat-PIC blobs differing only by the applier+data =="
function Build-Blob($src,$name){
    $obj = Join-Path $Out "tmp_$name.o"
    $a = $Common + @("-C","force-unwind-tables=no","-C","relocation-model=pic","-C","llvm-args=-min-jump-table-entries=200","--target","x86_64-unknown-linux-gnu","--emit=obj",$src,"-o",$obj)
    & rustc @a; if ($LASTEXITCODE -ne 0) { throw "rustc blob $name" }
    & $Lld -flavor gnu --oformat binary -T $FlatLd -o (Join-Path $Out $name) $obj
    if ($LASTEXITCODE -ne 0) { throw "lld $name" }
    return $obj
}
$op = Build-Blob "$S\pack\measure_patch_flat.rs"  "mx_patch_flat.bin"
Build-Blob "$S\pack\measure_driver_flat.rs" "mx_driver_flat.bin" | Out-Null
$xp = (Get-Item "$Out\mx_patch_flat.bin").Length
$xd = (Get-Item "$Out\mx_driver_flat.bin").Length
Write-Host ("   X_total (code+data, Q2口径) = {0} - {1} = {2} B   (Q2 X = 3003 B)" -f $xp,$xd,($xp-$xd))
Write-Host "== 6) code/data split (llvm-size on the applier object) =="
& $Size -A $op | Select-String "patch|Total"

Write-Host "== sizes =="
Get-ChildItem "$Out\A_st_*_windows.exe" | Select-Object Name,Length | Sort-Object Name | Format-Table -AutoSize
