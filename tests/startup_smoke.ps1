param(
    [string]$GuiExe = (Join-Path $PSScriptRoot '..\dist\agenterm.exe'),
    [string]$CliExe = (Join-Path $PSScriptRoot '..\dist\agentermctl.exe'),
    [int]$MaxWindowMs = 1000
)

$ErrorActionPreference = 'Stop'
$GuiExe = [IO.Path]::GetFullPath($GuiExe)
$CliExe = [IO.Path]::GetFullPath($CliExe)
foreach ($path in @($GuiExe, $CliExe)) {
    if (-not (Test-Path -LiteralPath $path)) {
        throw "AgenTerm executable not found: $path"
    }
}

& $CliExe kill-server 2>$null | Out-Null
Start-Sleep -Milliseconds 200

try {
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $process = Start-Process -FilePath $GuiExe -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds(5)
    $handle = [IntPtr]::Zero
    while ([DateTime]::UtcNow -lt $deadline) {
        $process.Refresh()
        $handle = $process.MainWindowHandle
        if ($handle -ne [IntPtr]::Zero) {
            break
        }
        Start-Sleep -Milliseconds 5
    }
    $watch.Stop()

    if ($handle -eq [IntPtr]::Zero) {
        throw 'AgenTerm did not expose its main window within 5 seconds.'
    }
    if ($watch.ElapsedMilliseconds -gt $MaxWindowMs) {
        throw "AgenTerm first window took $($watch.ElapsedMilliseconds) ms; limit is $MaxWindowMs ms."
    }

    & $CliExe wait-ui -t 0 --tab-state running --timeout-ms 10000 | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw 'The asynchronously started initial terminal did not become ready.'
    }

    $second = Start-Process -FilePath $GuiExe -PassThru
    if (-not $second.WaitForExit(1000)) {
        throw 'A second GUI launch did not hand off to the existing instance.'
    }

    Write-Host "PASS: first window in $($watch.ElapsedMilliseconds) ms; terminal loaded asynchronously"
}
finally {
    & $CliExe kill-server 2>$null | Out-Null
}
