# Q18 discovery experiment — build + run all Windows/x86_64 artifacts.
# Reuses Q3's blob sources (reuse/pack/ca) and hashes; the only new thing is the
# name->hash discovery layer in loader.rs (--cfg dc_discover). Produces a content
# store, two directories mapping the SAME name to DIFFERENT hashes, per-consumer
# trust.txt + a shared prog.txt, and both loader packings; then runs the ① gate.
$ErrorActionPreference = "Stop"

$Here   = Split-Path -Parent $MyInvocation.MyCommand.Path
$Root   = Split-Path -Parent $Here                 # research/dynamic-core/discovery
$Track  = Split-Path -Parent $Root                 # research/dynamic-core
$Reuse  = Join-Path $Track "reuse"
$Out    = Join-Path $Root "out"
$Store  = Join-Path $Out "store"
New-Item -ItemType Directory -Force -Path $Out   | Out-Null
New-Item -ItemType Directory -Force -Path $Store | Out-Null

$Sysroot    = (& rustc --print sysroot).Trim()
$HostTriple = (((& rustc -vV) | Select-String '^host: ').ToString() -split ' ')[1]
$Lld        = Join-Path $Sysroot "lib\rustlib\$HostTriple\bin\rust-lld.exe"

$WinTarget = "x86_64-pc-windows-msvc"
$ElfTarget = "x86_64-unknown-linux-gnu"
$Common = @("--edition","2021","-O","-C","panic=abort","-C","debuginfo=0","-A","unexpected_cfgs")
$FlatLd = Join-Path $Reuse "build\flat.ld"

Write-Host "== rustc: $((& rustc --version))"

function Build-Exe($src, $extraCfg, $name) {
    $a = $Common + $extraCfg + @(
        "--cfg","dc_os=`"windows`"","--target",$WinTarget,
        "-C","link-args=/subsystem:console /entry:mainCRTStartup /nodefaultlib /DEBUG:NONE",
        $src,"-o",(Join-Path $Out $name))
    & rustc @a
    if ($LASTEXITCODE -ne 0) { throw "rustc failed for $name" }
}
function Build-Blob($src, $name) {
    $obj = Join-Path $Out "tmp_$name.o"
    $a = $Common + @("-C","force-unwind-tables=no","-C","opt-level=2",
        "--cfg","dc_os=`"windows`"","-C","relocation-model=pic",
        "--target",$ElfTarget,"--emit=obj",$src,"-o",$obj)
    & rustc @a
    if ($LASTEXITCODE -ne 0) { throw "rustc failed for $name" }
    & $Lld -flavor gnu --oformat binary -T $FlatLd -o (Join-Path $Out $name) $obj
    if ($LASTEXITCODE -ne 0) { throw "lld failed for $name" }
    Remove-Item $obj -Force
}
function Fnv1a64Hex($path) {
    $bytes = [IO.File]::ReadAllBytes($path)
    $mask = [System.Numerics.BigInteger]::Parse("18446744073709551615")
    $h = [System.Numerics.BigInteger]::Parse("14695981039346656037")
    $p = [System.Numerics.BigInteger]::Parse("1099511628211")
    foreach ($b in $bytes) { $h = $h -bxor ([System.Numerics.BigInteger]$b); $h = ($h * $p) -band $mask }
    return ('{0:x16}' -f [uint64]$h)
}
function Store-Blob($blobName) {
    $src = Join-Path $Out $blobName
    $hash = Fnv1a64Hex $src
    Copy-Item $src (Join-Path $Store "$hash.bin") -Force
    return $hash
}

Write-Host "== build Q3 blobs into this experiment's store =="
Build-Blob "$Reuse\pack\ca\payload_readlen.rs" "ca_payload_readlen_windows.bin"
Build-Blob "$Reuse\pack\ca\adapter_v1.rs"      "ca_adapter_v1_windows.bin"
Build-Blob "$Reuse\pack\ca\adapter_v2.rs"      "ca_adapter_v2_windows.bin"

$H_payload = Store-Blob "ca_payload_readlen_windows.bin"
$H_v1      = Store-Blob "ca_adapter_v1_windows.bin"
$H_v2      = Store-Blob "ca_adapter_v2_windows.bin"
Write-Host "  payload_readlen = $H_payload"
Write-Host "  adapter_v1      = $H_v1  (full read)"
Write-Host "  adapter_v2      = $H_v2  (truncated read)"

Write-Host "== two INDEPENDENT directories: same name 'fileio' -> DIFFERENT hashes =="
# dir_a is authored by publisher A; dir_b by publisher B. Neither is 'the' directory.
"readlen $H_payload`nfileio $H_v1`n" | Set-Content -NoNewline (Join-Path $Out "dir_a.txt")
"readlen $H_payload`nfileio $H_v2`n" | Set-Content -NoNewline (Join-Path $Out "dir_b.txt")

Write-Host "== one shared program (names only), two consumers differ ONLY in trust.txt =="
"readlen`nfileio`n" | Set-Content -NoNewline (Join-Path $Out "prog.txt")
"dir_a.txt" | Set-Content -NoNewline (Join-Path $Out "trust_a.txt")
"dir_b.txt" | Set-Content -NoNewline (Join-Path $Out "trust_b.txt")

Write-Host "== candidate-3 baseline manifests (build-time-pinned hashes, ZERO discovery) =="
"$H_payload`n$H_v1" | Set-Content -NoNewline (Join-Path $Out "manifest_a.txt")
"$H_payload`n$H_v2" | Set-Content -NoNewline (Join-Path $Out "manifest_b.txt")

Write-Host "== loaders: baseline (hash) + discovery (name), same source, packed twice =="
Build-Exe "$Root\loader.rs" @()                      "loader_hash_windows.exe"
Build-Exe "$Root\loader.rs" @("--cfg","dc_discover") "loader_disc_windows.exe"

Write-Host "== sizes (bytes) =="
Get-ChildItem "$Out\loader_*windows.exe" | Select-Object Name, Length | Format-Table -AutoSize
$hashLen = (Get-Item (Join-Path $Out "loader_hash_windows.exe")).Length
$discLen = (Get-Item (Join-Path $Out "loader_disc_windows.exe")).Length
Write-Host "  discovery layer Δ (disc - hash) = $($discLen - $hashLen) bytes"

# ---- prepare a 35-byte input the readlen payload measures ----
[IO.File]::WriteAllText((Join-Path $Out "input.txt"), "dynamic-core experiment 2026-08-08`n")

Push-Location $Out
try {
    Write-Host "`n== (1) DISCOVERY: consumer A trusts dir_a (fileio->v1) =="
    Copy-Item "trust_a.txt" "trust.txt" -Force
    & ".\loader_disc_windows.exe"; Write-Host "   consumer A exit=$LASTEXITCODE (expect 0x23=35 => len=0023)"

    Write-Host "== (1) DISCOVERY: consumer B trusts dir_b (fileio->v2) =="
    Copy-Item "trust_b.txt" "trust.txt" -Force
    & ".\loader_disc_windows.exe"; Write-Host "   consumer B exit=$LASTEXITCODE (expect 0x08=8 => len=0008)"

    Write-Host "`n== (5) BUILD-TIME-PINNED baseline: same two outcomes, ZERO discovery code =="
    Copy-Item "manifest_a.txt" "manifest.txt" -Force
    & ".\loader_hash_windows.exe"; Write-Host "   pinned A exit=$LASTEXITCODE"
    Copy-Item "manifest_b.txt" "manifest.txt" -Force
    & ".\loader_hash_windows.exe"; Write-Host "   pinned B exit=$LASTEXITCODE"
}
finally { Pop-Location }
Write-Host "`n== done =="
