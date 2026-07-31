[CmdletBinding()]
param(
    [string]$OutputPath = (Join-Path $PSScriptRoot '..\evidence\local\windows-x86_64.json')
)

$ErrorActionPreference = 'Stop'
$workspace = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$target = Join-Path $workspace 'target\release'
$launcher = Join-Path $target 'agenterm-cc-web.exe'
$directHostExe = Join-Path $target 'agenterm-cc-web-direct-wry.exe'

function Invoke-TimedCargoBuild([string]$Package) {
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    & cargo build --release --locked -p $Package
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed for $Package" }
    $watch.Stop()
    return $watch.ElapsedMilliseconds
}

Push-Location $workspace
try {
    $hostBuildMs = Invoke-TimedCargoBuild 'agenterm-cc-web-direct-wry'
    $launcherBuildMs = Invoke-TimedCargoBuild 'agenterm-cc-web'

    $probeText = & $launcher --probe
    if ($LASTEXITCODE -ne 0) { throw "runtime probe failed: $probeText" }
    $probe = $probeText | ConvertFrom-Json

    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $directHostExe
    $startInfo.ArgumentList.Add('--smoke')
    $startInfo.ArgumentList.Add('--no-activate')
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    if (-not $process.Start()) { throw 'failed to start smoke process' }
    $peakWorkingSet = 0L
    $deadline = [DateTimeOffset]::UtcNow.AddSeconds(30)
    while (-not $process.WaitForExit(50)) {
        $process.Refresh()
        $peakWorkingSet = [Math]::Max($peakWorkingSet, $process.WorkingSet64)
        if ([DateTimeOffset]::UtcNow -gt $deadline) {
            $process.Kill($true)
            throw 'event-driven smoke exceeded 30 second deadline'
        }
    }
    $watch.Stop()
    $stdout = $process.StandardOutput.ReadToEnd()
    $stderr = $process.StandardError.ReadToEnd()
    if ($process.ExitCode -ne 0) { throw "smoke failed: $stdout $stderr" }
    $smoke = $stdout | ConvertFrom-Json

    $receipt = [ordered]@{
        schema = 'agenterm.webview-measurement/1'
        observed_at_utc = [DateTimeOffset]::UtcNow.ToString('O')
        platform = 'windows-x86_64'
        rustc = (& rustc --version)
        cargo = (& cargo --version)
        build_ms = [ordered]@{ direct_wry_host = $hostBuildMs; fallback_launcher = $launcherBuildMs }
        binary_bytes = [ordered]@{ direct_wry_host = (Get-Item $directHostExe).Length; fallback_launcher = (Get-Item $launcher).Length }
        probe = $probe
        smoke = $smoke
        smoke_elapsed_ms = $watch.ElapsedMilliseconds
        direct_host_peak_working_set_bytes = $peakWorkingSet
        limitations = @('warm/cold state is operator-controlled', 'working set excludes WebView child processes', 'page load is not first paint')
    }
    $directory = Split-Path -Parent $OutputPath
    New-Item -ItemType Directory -Force -Path $directory | Out-Null
    $receipt | ConvertTo-Json -Depth 8 | Set-Content -Encoding utf8 -Path $OutputPath
    Write-Output $OutputPath
} finally {
    Pop-Location
}
