# Build and run the neutral-IR experiment (Windows). Raw rustc, no Cargo, no workspace.
$ErrorActionPreference = "Stop"
$Here = Split-Path -Parent $MyInvocation.MyCommand.Path
$Root = Split-Path -Parent $Here   # research/dynamic-core/ir
$Out  = Join-Path $Root "out"
New-Item -ItemType Directory -Force -Path $Out | Out-Null

Write-Host "== rustc: $((& rustc --version))"
& rustc --edition 2021 -O -A dead_code (Join-Path $Root "main.rs") -o (Join-Path $Out "driver.exe")
if ($LASTEXITCODE -ne 0) { throw "rustc failed" }

Push-Location $Out
try { & .\driver.exe } finally { Pop-Location }
