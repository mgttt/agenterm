# Build all Windows/x86_64 artifacts for the adapter-reuse (Q5) experiment.
# Produces: baked baseline blobs, content-addressed (CA) blobs + a content store,
# the CA loader (verify + non-verify) and the embed baseline loader, and per-program
# manifests. Flat blobs are compiled for the ELF/sysv64 target (clean extraction) and
# loaded uniformly; the native PE loader bridges sysv64 -> win64 when calling the OS.
$ErrorActionPreference = "Stop"

$Here = Split-Path -Parent $MyInvocation.MyCommand.Path
$Root = Split-Path -Parent $Here
$Out  = Join-Path $Root "out"
$Store = Join-Path $Out "store"
New-Item -ItemType Directory -Force -Path $Out | Out-Null
New-Item -ItemType Directory -Force -Path $Store | Out-Null

$Sysroot = (& rustc --print sysroot).Trim()
$HostTriple = (((& rustc -vV) | Select-String '^host: ').ToString() -split ' ')[1]
$Lld = Join-Path $Sysroot "lib\rustlib\$HostTriple\bin\rust-lld.exe"

$WinTarget = "x86_64-pc-windows-msvc"
$ElfTarget = "x86_64-unknown-linux-gnu"
$Common = @("--edition", "2021", "-O", "-C", "panic=abort", "-C", "debuginfo=0", "-A", "unexpected_cfgs")

Write-Host "== rustc: $((& rustc --version))"

function Build-Exe($src, $extraCfg, $name) {
    $a = $Common + $extraCfg + @(
        "--cfg", "dc_os=`"windows`"",
        "--target", $WinTarget,
        "-C", "link-args=/subsystem:console /entry:mainCRTStartup /nodefaultlib /DEBUG:NONE",
        $src, "-o", (Join-Path $Out $name)
    )
    & rustc @a
    if ($LASTEXITCODE -ne 0) { throw "rustc failed for $name" }
}

function Build-Blob($src, $name, $opt = "2") {
    $obj = Join-Path $Out "tmp.o"
    $a = $Common + @(
        "-C", "force-unwind-tables=no", "-C", "opt-level=$opt",
        "--cfg", "dc_os=`"windows`"", "-C", "relocation-model=pic",
        "--target", $ElfTarget, "--emit=obj", $src, "-o", $obj
    )
    & rustc @a
    if ($LASTEXITCODE -ne 0) { throw "rustc failed for $name" }
    & $Lld -flavor gnu --oformat binary -T (Join-Path $Here "flat.ld") -o (Join-Path $Out $name) $obj
    if ($LASTEXITCODE -ne 0) { throw "lld failed for $name" }
    Remove-Item $obj -Force
}

# FNV-1a/64 of a file's bytes -> 16-char lowercase hex (the content address).
function Fnv1a64Hex($path) {
    $bytes = [IO.File]::ReadAllBytes($path)
    $mask = [System.Numerics.BigInteger]::Parse("18446744073709551615") # 2^64-1
    $h = [System.Numerics.BigInteger]::Parse("14695981039346656037")   # offset basis
    $p = [System.Numerics.BigInteger]::Parse("1099511628211")          # FNV prime
    foreach ($b in $bytes) {
        $h = $h -bxor ([System.Numerics.BigInteger]$b)
        $h = ($h * $p) -band $mask
    }
    return ('{0:x16}' -f [uint64]$h)
}

# Store a blob by content address; returns its hash. Demonstrates dedup: identical
# content -> identical filename -> one file.
function Store-Blob($blobName) {
    $src = Join-Path $Out $blobName
    $hash = Fnv1a64Hex $src
    Copy-Item $src (Join-Path $Store "$hash.bin") -Force
    return $hash
}

Write-Host "== baked baseline blobs (adapter compiled into each payload) =="
Build-Blob "$Root\pack\baked\rhp.rs"     "baked_rhp_windows.bin"
Build-Blob "$Root\pack\baked\readlen.rs" "baked_readlen_windows.bin"

Write-Host "== content-addressed blobs =="
Build-Blob "$Root\pack\ca\adapter_v1.rs"      "ca_adapter_v1_windows.bin"
Build-Blob "$Root\pack\ca\adapter_v2.rs"      "ca_adapter_v2_windows.bin"
Build-Blob "$Root\pack\ca\payload_rhp.rs"     "ca_payload_rhp_windows.bin"
Build-Blob "$Root\pack\ca\payload_readlen.rs" "ca_payload_readlen_windows.bin"
# criterion ④(b): same source, different opt level (tests build determinism).
Build-Blob "$Root\pack\ca\adapter_v1.rs"      "ca_adapter_v1_opt1_windows.bin" "1"
# criterion ④(b): DIFFERENT source, SAME behavior (loop read) => different bytes.
Build-Blob "$Root\pack\ca\adapter_v1alt.rs"   "ca_adapter_v1alt_windows.bin"

Write-Host "== populate content store (content-addressed) =="
$H_adapter_v1 = Store-Blob "ca_adapter_v1_windows.bin"
$H_adapter_v2 = Store-Blob "ca_adapter_v2_windows.bin"
$H_payload_rhp = Store-Blob "ca_payload_rhp_windows.bin"
$H_payload_readlen = Store-Blob "ca_payload_readlen_windows.bin"
$H_adapter_v1_opt1 = Fnv1a64Hex (Join-Path $Out "ca_adapter_v1_opt1_windows.bin")
$H_adapter_v1alt = Fnv1a64Hex (Join-Path $Out "ca_adapter_v1alt_windows.bin")

Write-Host "  adapter_v1         = $H_adapter_v1"
Write-Host "  adapter_v2         = $H_adapter_v2"
Write-Host "  payload_rhp        = $H_payload_rhp"
Write-Host "  payload_readlen    = $H_payload_readlen"
Write-Host "  adapter_v1(opt=1)  = $H_adapter_v1_opt1  (SAME source, diff flags: determinism => same hash expected)"
Write-Host "  adapter_v1alt      = $H_adapter_v1alt  (DIFF source, SAME behavior: dedup FAILS => diff hash expected)"

Write-Host "== manifests (one per program; content-addressed bill of materials) =="
# program = payload (line 1) + adapters (rest). rhp+readlen share adapter_v1's hash.
"$H_payload_rhp`n$H_adapter_v1"     | Set-Content -NoNewline (Join-Path $Out "manifest_rhp.txt")
"$H_payload_readlen`n$H_adapter_v1" | Set-Content -NoNewline (Join-Path $Out "manifest_readlen.txt")
# criterion ②: readlen bound to the INCOMPATIBLE v2 adapter (truncated read).
"$H_payload_readlen`n$H_adapter_v2" | Set-Content -NoNewline (Join-Path $Out "manifest_readlen_v2.txt")

Write-Host "== CA loaders (mechanism under test) =="
Build-Exe "$Root\loader.rs" @()                          "loader_ca_windows.exe"
Build-Exe "$Root\loader.rs" @("--cfg", "dc_verify")      "loader_ca_verify_windows.exe"

Write-Host "== embed baseline loader (Q0 variant B; ③ reference) =="
$env:DC_BLOB = (Join-Path $Out "baked_rhp_windows.bin")
Build-Exe "$Root\loader_embed.rs" @() "loader_embed_rhp_windows.exe"
Remove-Item Env:\DC_BLOB

Write-Host "== done. sizes (bytes): =="
Get-ChildItem "$Out\*windows*" | Sort-Object Name | Select-Object Name, Length | Format-Table -AutoSize
Write-Host "== store contents (content-addressed; note dedup): =="
Get-ChildItem "$Store\*.bin" | Select-Object Name, Length | Format-Table -AutoSize
