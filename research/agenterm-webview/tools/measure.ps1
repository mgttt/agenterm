[CmdletBinding()]
param(
    [string]$OutputPath = (Join-Path $PSScriptRoot '..\evidence\local\windows-comparison.json'),
    [string]$NativeControlCenterPath = (Join-Path $PSScriptRoot '..\..\..\dist\agenterm-cc.exe'),
    [ValidateRange(1, 10)][int]$RepeatedStartupSamples = 3
)

$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'This measurement slice is Windows-only' }

$workspace = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$tauriWorkspace = Join-Path $workspace 'tauri-reference'
$runId = [DateTimeOffset]::UtcNow.ToString('yyyyMMdd-HHmmss-fff')
$localRoot = Join-Path $workspace 'evidence\local'
$runRoot = Join-Path $localRoot "runs\$runId"
$directTarget = Join-Path $runRoot 'target-direct-wry'
$tauriTarget = Join-Path $runRoot 'target-tauri'
$launcher = Join-Path $directTarget 'release\agenterm-cc-web.exe'
$directHost = Join-Path $directTarget 'release\agenterm-cc-web-direct-wry.exe'
$tauriHost = Join-Path $tauriTarget 'release\agenterm-cc-web-tauri.exe'

function Invoke-TimedBuild(
    [string]$WorkingDirectory,
    [string]$ManifestPath,
    [string]$Package,
    [string]$TargetDirectory
) {
    Push-Location $WorkingDirectory
    try {
        $watch = [System.Diagnostics.Stopwatch]::StartNew()
        & cargo build --release --locked --manifest-path $ManifestPath --target-dir $TargetDirectory -p $Package
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed for $Package" }
        $watch.Stop()
        return $watch.ElapsedMilliseconds
    } finally {
        Pop-Location
    }
}

function Get-CargoInventory([string]$WorkingDirectory, [string]$ManifestPath) {
    Push-Location $WorkingDirectory
    try {
        $metadataText = & cargo metadata --locked --format-version 1 --manifest-path $ManifestPath
        if ($LASTEXITCODE -ne 0) { throw "cargo metadata failed for $ManifestPath" }
        $metadata = $metadataText | ConvertFrom-Json
        $packages = @($metadata.packages | Sort-Object name, version)
        return [ordered]@{
            package_count = $packages.Count
            missing_license_count = @($packages | Where-Object { [string]::IsNullOrWhiteSpace($_.license) }).Count
            license_expressions = @($packages | ForEach-Object license | Where-Object { $_ } | Sort-Object -Unique)
        }
    } finally {
        Pop-Location
    }
}

function Invoke-HostSample([string]$Executable) {
    $startInfo = [System.Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $Executable
    $startInfo.ArgumentList.Add('--smoke')
    $startInfo.ArgumentList.Add('--no-activate')
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $process = [System.Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    if (-not $process.Start()) { throw "failed to start $Executable" }
    if (-not $process.WaitForExit(30000)) {
        $process.Kill($true)
        $process.WaitForExit()
        throw "event-driven smoke exceeded 30 seconds: $Executable"
    }
    $watch.Stop()
    $stdout = $process.StandardOutput.ReadToEnd()
    $stderr = $process.StandardError.ReadToEnd()
    if ($process.ExitCode -ne 0) { throw "smoke failed: $stdout $stderr" }
    $process.Refresh()
    return [ordered]@{
        wall_ms = $watch.ElapsedMilliseconds
        root_process_peak_working_set_bytes = $process.PeakWorkingSet64
        receipt = ($stdout | ConvertFrom-Json)
    }
}

function Invoke-LauncherProbe([string]$Implementation) {
    $text = & $launcher --implementation $Implementation --probe --no-activate
    if ($LASTEXITCODE -ne 0) { throw "launcher probe failed for $Implementation`: $text" }
    return ($text | ConvertFrom-Json)
}

function New-ComparisonArchive(
    [string]$Name,
    [System.Collections.Generic.List[string]]$Files
) {
    $stage = Join-Path $runRoot "stage-$Name"
    $archive = Join-Path $runRoot "$Name.zip"
    New-Item -ItemType Directory -Force -Path $stage | Out-Null
    foreach ($file in $Files) { Copy-Item -LiteralPath $file -Destination $stage }
    Get-ChildItem -LiteralPath $stage -File | Compress-Archive -DestinationPath $archive
    return [ordered]@{
        bytes = (Get-Item -LiteralPath $archive).Length
        sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $archive).Hash.ToLowerInvariant()
        contents = @((Get-ChildItem -LiteralPath $stage -File).Name | Sort-Object)
    }
}

New-Item -ItemType Directory -Force -Path $runRoot | Out-Null
$primaryManifest = Join-Path $workspace 'Cargo.toml'
$tauriManifest = Join-Path $tauriWorkspace 'Cargo.toml'

$directColdBuildMs = Invoke-TimedBuild $workspace $primaryManifest 'agenterm-cc-web-direct-wry' $directTarget
$directWarmBuildMs = Invoke-TimedBuild $workspace $primaryManifest 'agenterm-cc-web-direct-wry' $directTarget
$launcherBuildMs = Invoke-TimedBuild $workspace $primaryManifest 'agenterm-cc-web' $directTarget
$tauriColdBuildMs = Invoke-TimedBuild $tauriWorkspace $tauriManifest 'agenterm-cc-web-tauri' $tauriTarget
$tauriWarmBuildMs = Invoke-TimedBuild $tauriWorkspace $tauriManifest 'agenterm-cc-web-tauri' $tauriTarget

# Stage the isolated Tauri reference beside the no-runtime launcher so both
# implementations traverse the identical external fallback boundary.
Copy-Item -LiteralPath $tauriHost -Destination (Join-Path $directTarget 'release\agenterm-cc-web-tauri.exe')

$directProbe = Invoke-LauncherProbe 'direct-wry'
$tauriProbe = Invoke-LauncherProbe 'tauri'
$assetManifestText = & $launcher --asset-manifest
if ($LASTEXITCODE -ne 0) { throw 'asset manifest failed' }
$assetManifest = $assetManifestText | ConvertFrom-Json

$directFirst = Invoke-HostSample $directHost
$tauriFirst = Invoke-HostSample $tauriHost
$directRepeated = @()
$tauriRepeated = @()
for ($index = 0; $index -lt $RepeatedStartupSamples; $index++) {
    $directRepeated += Invoke-HostSample $directHost
    $tauriRepeated += Invoke-HostSample $tauriHost
}

$directArchiveFiles = [System.Collections.Generic.List[string]]::new()
$directArchiveFiles.Add($launcher)
$directArchiveFiles.Add($directHost)
$tauriArchiveFiles = [System.Collections.Generic.List[string]]::new()
$tauriArchiveFiles.Add($launcher)
$tauriArchiveFiles.Add($tauriHost)
$directArchive = New-ComparisonArchive 'direct-wry' $directArchiveFiles
$tauriArchive = New-ComparisonArchive 'tauri-v2' $tauriArchiveFiles

$native = [ordered]@{
    status = 'unavailable'
    reason = 'native Control Center artifact was not found; startup/RSS were intentionally not inferred'
    executable_bytes = $null
    archive = $null
    startup = $null
}
if (Test-Path -LiteralPath $NativeControlCenterPath -PathType Leaf) {
    $nativeFiles = [System.Collections.Generic.List[string]]::new()
    $nativeFiles.Add((Resolve-Path $NativeControlCenterPath).Path)
    $native = [ordered]@{
        status = 'artifact-only'
        reason = 'binary/archive measured; native startup and RSS require its owning isolated public journey'
        executable_bytes = (Get-Item -LiteralPath $NativeControlCenterPath).Length
        archive = New-ComparisonArchive 'native-cc' $nativeFiles
        startup = $null
    }
}

$receipt = [ordered]@{
    schema = 'agenterm.webview-comparison/1'
    observed_at_utc = [DateTimeOffset]::UtcNow.ToString('O')
    platform = 'windows-x86_64'
    rustc = (& rustc --version)
    cargo = (& cargo --version)
    active_renderer = 'native'
    asset_manifest = $assetManifest
    runtime_policy = [ordered]@{
        system_runtime_only = $true
        runtime_download = $false
        fixed_runtime_bundled = $false
    }
    native_control_center = $native
    direct_wry = [ordered]@{
        implementation = 'direct-wry'
        versions = [ordered]@{ wry = '0.56.0'; tao = '0.36.0' }
        javascript_toolchain = 'none'
        build_ms = [ordered]@{ empty_target = $directColdBuildMs; warm_unchanged = $directWarmBuildMs; launcher = $launcherBuildMs }
        executable_bytes = (Get-Item -LiteralPath $directHost).Length
        fallback_launcher_bytes = (Get-Item -LiteralPath $launcher).Length
        archive = $directArchive
        inventory = Get-CargoInventory $workspace $primaryManifest
        lock_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $workspace 'Cargo.lock')).Hash.ToLowerInvariant()
        probe = $directProbe
        first_observed_start = $directFirst
        repeated_start_samples = $directRepeated
    }
    tauri_v2 = [ordered]@{
        implementation = 'tauri-v2-reference'
        versions = [ordered]@{ tauri = '2.11.5'; tauri_build = '2.6.3' }
        javascript_toolchain = 'none'
        registered_commands = 0
        registered_plugins = 0
        capabilities = 0
        build_ms = [ordered]@{ empty_target = $tauriColdBuildMs; warm_unchanged = $tauriWarmBuildMs }
        executable_bytes = (Get-Item -LiteralPath $tauriHost).Length
        fallback_launcher_bytes = (Get-Item -LiteralPath $launcher).Length
        archive = $tauriArchive
        inventory = Get-CargoInventory $tauriWorkspace $tauriManifest
        lock_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $tauriWorkspace 'Cargo.lock')).Hash.ToLowerInvariant()
        probe = $tauriProbe
        first_observed_start = $tauriFirst
        repeated_start_samples = $tauriRepeated
    }
    unavailable_measurements = [ordered]@{
        controlled_cold_start_ms = $null
        first_paint_ms = $null
        full_webview_process_tree_rss_bytes = $null
        installer_bytes = $null
        intentionally_missing_webview2_runtime = $null
        dpi_png = $null
        crash_reload = $null
        foreground_preservation = $null
    }
    limitations = @(
        'empty_target build is a Rust target-cache measurement, not a cold OS or registry download measurement',
        'first_observed_start is not called cold because the OS/WebView caches were not reset',
        'load_complete is not first paint',
        'root process peak working set excludes WebView child processes',
        'native Control Center startup/RSS remains owned by its isolated public journey',
        'licence expressions are metadata inventory and not a completed legal/SBOM review',
        'this Windows reference does not provide macOS or Linux renderer evidence'
    )
}

$directory = Split-Path -Parent $OutputPath
New-Item -ItemType Directory -Force -Path $directory | Out-Null
$receipt | ConvertTo-Json -Depth 12 | Set-Content -Encoding utf8 -Path $OutputPath
Write-Output $OutputPath
