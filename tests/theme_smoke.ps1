param(
    [string]$GuiExe = (Join-Path $PSScriptRoot '..\dist\agenterm.exe'),
    [string]$CliExe = (Join-Path $PSScriptRoot '..\dist\agenterm-cli.exe'),
    [switch]$ListEvidence
)

$ErrorActionPreference = 'Stop'
$declaredEvidence = @('ux.theme-settings')
if ($ListEvidence) {
    $declaredEvidence
    exit 0
}

function Write-Evidence {
    param([Parameter(Mandatory = $true)][string]$Id)
    if ($declaredEvidence -notcontains $Id) {
        throw "Theme smoke emitted undeclared evidence ID: $Id"
    }
    Write-Host "EVIDENCE $Id"
}

$GuiExe = [IO.Path]::GetFullPath($GuiExe)
$CliExe = [IO.Path]::GetFullPath($CliExe)
foreach ($path in @($GuiExe, $CliExe)) {
    if (-not (Test-Path -LiteralPath $path)) {
        throw "AgenTerm executable not found: $path"
    }
}

$previousAddress = $env:AGENTERM_IPC_ADDRESS
$previousWorkspace = $env:AGENTERM_WORKSPACE_PATH
$previousSettings = $env:AGENTERM_SETTINGS_PATH
$listener = [Net.Sockets.TcpListener]::new(
    [Net.IPAddress]::Loopback,
    0
)
$listener.Start()
$port = ([Net.IPEndPoint]$listener.LocalEndpoint).Port
$listener.Stop()
$env:AGENTERM_IPC_ADDRESS = "127.0.0.1:$port"
$workspaceFile = Join-Path $env:TEMP "agenterm-theme-workspace-$PID.json"
$settingsFile = Join-Path $env:TEMP "agenterm-theme-settings-$PID.json"
$stderrFile = Join-Path $env:TEMP "agenterm-theme-stderr-$PID.txt"
$darkPng = Join-Path $env:TEMP "agenterm-theme-dark-$PID.png"
$lightPng = Join-Path $env:TEMP "agenterm-theme-light-$PID.png"
$env:AGENTERM_WORKSPACE_PATH = $workspaceFile
$env:AGENTERM_SETTINGS_PATH = $settingsFile
$guiProcesses = [Collections.Generic.List[Diagnostics.Process]]::new()

function Invoke-AgenTerm {
    param([string[]]$CommandArgs)
    $previousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $output = & $CliExe @CommandArgs 2>&1
        $exitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previousPreference
    }
    if ($exitCode -ne 0) {
        throw "agenterm-cli $($CommandArgs -join ' ') failed:`n$($output -join "`n")"
    }
    return ($output -join "`n")
}

function Test-AgenTermReady {
    $previousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $null = & $CliExe ui-snapshot 2>&1
        return $LASTEXITCODE -eq 0
    }
    finally {
        $ErrorActionPreference = $previousPreference
    }
}

function Wait-AgenTermReady {
    param([bool]$Expected = $true)
    for ($attempt = 0; $attempt -lt 200; $attempt++) {
        if ((Test-AgenTermReady) -eq $Expected) {
            return
        }
        Start-Sleep -Milliseconds 25
    }
    throw "AgenTerm readiness did not become '$Expected'"
}

function Start-IsolatedGui {
    if (Test-Path -LiteralPath $stderrFile) {
        Remove-Item -LiteralPath $stderrFile -Force
    }
    $process = Start-Process -FilePath $GuiExe `
        -RedirectStandardError $stderrFile `
        -PassThru
    $guiProcesses.Add($process)
    Wait-AgenTermReady
    for ($attempt = 0; $attempt -lt 200; $attempt++) {
        $process.Refresh()
        if ($process.MainWindowHandle -ne [IntPtr]::Zero) {
            return $process
        }
        Start-Sleep -Milliseconds 25
    }
    throw 'AgenTerm did not expose its native window'
}

function Get-OnlyTab {
    $snapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $tabs = @($snapshot.tabs)
    if ($tabs.Count -ne 1) {
        throw "Expected one isolated tab, found $($tabs.Count)"
    }
    return $tabs[0]
}

function Get-Pane {
    param([Parameter(Mandatory = $true)][string]$Target)
    $paneSnapshot = Invoke-AgenTerm @(
        'pane-snapshot', '-t', $Target
    ) | ConvertFrom-Json
    $panes = @($paneSnapshot.windows)
    if ($panes.Count -ne 1) {
        throw "Expected one pane for $Target, found $($panes.Count)"
    }
    return $panes[0]
}

function Assert-ThemeSnapshot {
    param(
        [Parameter(Mandatory = $true)]$Snapshot,
        [Parameter(Mandatory = $true)][string]$Saved,
        [AllowNull()][string]$Draft,
        [bool]$Open
    )
    if ($Snapshot.settings.color_theme -ne $Saved) {
        throw "Expected saved theme '$Saved', got '$($Snapshot.settings.color_theme)'"
    }
    if ($Open) {
        if ($Snapshot.modal.kind -ne 'settings' -or
            $Snapshot.settings.theme_draft -ne $Draft) {
            throw "Expected open Settings with '$Draft' draft"
        }
    } elseif ($null -ne $Snapshot.modal -or
        $null -ne $Snapshot.settings.theme_draft) {
        throw 'Expected Settings to be closed without a draft'
    }
    $options = @($Snapshot.settings.theme_options)
    if ($options.Count -ne 2 -or
        -not ($options | Where-Object { $_.id -eq 'dark' -and $_.label -eq 'Dark' }) -or
        -not ($options | Where-Object { $_.id -eq 'light' -and $_.label -eq 'Light' })) {
        throw 'Theme options were not exposed as stable dark/light IDs and labels'
    }
}

Add-Type -AssemblyName System.Drawing
function Get-PngSummary {
    param([Parameter(Mandatory = $true)][string]$Path)
    if (-not (Test-Path -LiteralPath $Path) -or
        (Get-Item -LiteralPath $Path).Length -lt 1000) {
        throw "Screenshot was not created correctly: $Path"
    }
    $bitmap = [Drawing.Bitmap]::new($Path)
    try {
        [long]$red = 0
        [long]$green = 0
        [long]$blue = 0
        [long]$samples = 0
        $stepX = [Math]::Max(1, [int]($bitmap.Width / 64))
        $stepY = [Math]::Max(1, [int]($bitmap.Height / 64))
        for ($y = 0; $y -lt $bitmap.Height; $y += $stepY) {
            for ($x = 0; $x -lt $bitmap.Width; $x += $stepX) {
                $pixel = $bitmap.GetPixel($x, $y)
                $red += $pixel.R
                $green += $pixel.G
                $blue += $pixel.B
                $samples++
            }
        }
        return [pscustomobject]@{
            Width = $bitmap.Width
            Height = $bitmap.Height
            Samples = $samples
            Red = [Math]::Round($red / $samples, 2)
            Green = [Math]::Round($green / $samples, 2)
            Blue = [Math]::Round($blue / $samples, 2)
            Luminance = [Math]::Round(
                (0.2126 * $red + 0.7152 * $green + 0.0722 * $blue) / $samples,
                2
            )
        }
    }
    finally {
        $bitmap.Dispose()
    }
}

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class AgenTermThemeNativeTest {
    [DllImport("user32.dll")]
    private static extern IntPtr GetDlgItem(IntPtr parent, int id);

    [DllImport("user32.dll")]
    private static extern IntPtr SendMessageW(
        IntPtr window, uint message, UIntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    private static extern bool PostMessageW(
        IntPtr window, uint message, UIntPtr wParam, IntPtr lParam);

    public static void ClickControl(IntPtr parent, int id) {
        IntPtr control = GetDlgItem(parent, id);
        if (control == IntPtr.Zero) {
            throw new InvalidOperationException(
                "GetDlgItem could not find control " + id);
        }
        SendMessageW(control, 0x00F5, UIntPtr.Zero, IntPtr.Zero);
    }

    public static void Escape(IntPtr window) {
        if (!PostMessageW(window, 0x0100, (UIntPtr)0x1B, IntPtr.Zero) ||
            !PostMessageW(window, 0x0101, (UIntPtr)0x1B, IntPtr.Zero)) {
            throw new InvalidOperationException("Could not post Escape");
        }
    }
}
'@

try {
    Write-Host 'STEP launch isolated Dark workspace and keep a PTY active'
    try {
        & $CliExe kill-server 2>$null | Out-Null
    }
    catch {
        # A fresh isolated address normally has no server to stop.
    }
    $process = Start-IsolatedGui
    $tab = Get-OnlyTab
    Invoke-AgenTerm @(
        'wait-ui', '-t', $tab.id, '--tab-state', 'running', '--timeout-ms', '10000'
    ) | Out-Null
    $paneBefore = Get-Pane -Target $tab.id
    $tokenBefore = "AGENTERM_THEME_BEFORE_$PID"
    Invoke-AgenTerm @('send-keys', '-t', $tab.id, "echo $tokenBefore", 'Enter') | Out-Null
    Invoke-AgenTerm @(
        'wait-pane', '-t', $tab.id, '--contains', $tokenBefore,
        '--submit-complete', '--timeout-ms', '10000'
    ) | Out-Null

    Write-Host 'STEP native Light preview and Cancel rollback'
    $snapshot = Invoke-AgenTerm @('ui-action', 'open-settings') | ConvertFrom-Json
    Assert-ThemeSnapshot -Snapshot $snapshot -Saved dark -Draft dark -Open $true
    Invoke-AgenTerm @('screenshot', '-o', $darkPng) | Out-Null
    [AgenTermThemeNativeTest]::ClickControl($process.MainWindowHandle, 1009)
    $snapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    Assert-ThemeSnapshot -Snapshot $snapshot -Saved dark -Draft light -Open $true
    Invoke-AgenTerm @('screenshot', '-o', $lightPng) | Out-Null
    [AgenTermThemeNativeTest]::ClickControl($process.MainWindowHandle, 1010)
    $snapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    Assert-ThemeSnapshot -Snapshot $snapshot -Saved dark -Draft $null -Open $false

    Write-Host 'STEP Escape rolls a second Light preview back'
    Invoke-AgenTerm @('ui-action', 'open-settings') | Out-Null
    [AgenTermThemeNativeTest]::ClickControl($process.MainWindowHandle, 1009)
    $snapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    Assert-ThemeSnapshot -Snapshot $snapshot -Saved dark -Draft light -Open $true
    [AgenTermThemeNativeTest]::Escape($process.MainWindowHandle)
    for ($attempt = 0; $attempt -lt 100; $attempt++) {
        $snapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
        if ($null -eq $snapshot.modal) {
            break
        }
        Start-Sleep -Milliseconds 10
    }
    Assert-ThemeSnapshot -Snapshot $snapshot -Saved dark -Draft $null -Open $false

    Write-Host 'STEP native Apply persists Light without interrupting the PTY'
    Invoke-AgenTerm @('ui-action', 'open-settings') | Out-Null
    [AgenTermThemeNativeTest]::ClickControl($process.MainWindowHandle, 1009)
    [AgenTermThemeNativeTest]::ClickControl($process.MainWindowHandle, 1006)
    $snapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    Assert-ThemeSnapshot -Snapshot $snapshot -Saved light -Draft $null -Open $false
    if (-not (Test-Path -LiteralPath $settingsFile) -or
        (Get-Content -LiteralPath $settingsFile -Raw | ConvertFrom-Json).color_theme -ne 'light') {
        throw 'Apply did not persist the Light theme to the isolated settings file'
    }
    $tokenAfter = "AGENTERM_THEME_AFTER_$PID"
    Invoke-AgenTerm @('send-keys', '-t', $tab.id, "echo $tokenAfter", 'Enter') | Out-Null
    Invoke-AgenTerm @(
        'wait-pane', '-t', $tab.id, '--contains', $tokenAfter,
        '--submit-complete', '--timeout-ms', '10000'
    ) | Out-Null
    $paneAfter = Get-Pane -Target $tab.id
    $capture = Invoke-AgenTerm @('capture-pane', '-p', '-t', $tab.id)
    if ($paneAfter.pid -ne $paneBefore.pid -or
        $paneAfter.dead -or
        $paneAfter.output_bytes -lt $paneBefore.output_bytes -or
        -not $capture.Contains($tokenBefore) -or
        -not $capture.Contains($tokenAfter)) {
        throw 'Theme preview/apply interrupted the active PTY or its output'
    }

    Write-Host 'STEP Dark and Light previews have distinct rendered pixels'
    $dark = Get-PngSummary -Path $darkPng
    $light = Get-PngSummary -Path $lightPng
    if ($dark.Width -ne $light.Width -or
        $dark.Height -ne $light.Height -or
        [Math]::Abs($dark.Luminance - $light.Luminance) -lt 20) {
        throw "Theme screenshots were not visibly distinct: dark=$($dark | ConvertTo-Json -Compress) light=$($light | ConvertTo-Json -Compress)"
    }
    Write-Host "Dark screenshot:  $darkPng ($($dark.Width)x$($dark.Height), luminance $($dark.Luminance))"
    Write-Host "Light screenshot: $lightPng ($($light.Width)x$($light.Height), luminance $($light.Luminance))"

    Write-Host 'STEP saved Light theme survives shutdown and restart'
    Invoke-AgenTerm @('shutdown') | Out-Null
    Wait-AgenTermReady -Expected $false
    $process = Start-IsolatedGui
    $restored = Get-OnlyTab
    Invoke-AgenTerm @(
        'wait-ui', '-t', $restored.id, '--tab-state', 'running', '--timeout-ms', '10000'
    ) | Out-Null
    $snapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    Assert-ThemeSnapshot -Snapshot $snapshot -Saved light -Draft $null -Open $false

    Write-Evidence 'ux.theme-settings'
    Write-Host 'PASS: theme preview, rollback, persistence, PTY continuity, and rendering'
}
finally {
    try {
        & $CliExe kill-server 2>$null | Out-Null
    }
    catch {
        # Cleanup is best-effort if the isolated server already stopped.
    }
    foreach ($process in $guiProcesses) {
        if ($null -ne $process -and -not $process.HasExited) {
            $process.Kill()
            $process.WaitForExit()
        }
        if ($null -ne $process) {
            $process.Dispose()
        }
    }
    Remove-Item -LiteralPath $workspaceFile -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $settingsFile -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $stderrFile -ErrorAction SilentlyContinue
    # PNGs intentionally remain in TEMP for manual visual inspection.
    if ($null -eq $previousAddress) {
        Remove-Item Env:AGENTERM_IPC_ADDRESS -ErrorAction SilentlyContinue
    } else {
        $env:AGENTERM_IPC_ADDRESS = $previousAddress
    }
    if ($null -eq $previousWorkspace) {
        Remove-Item Env:AGENTERM_WORKSPACE_PATH -ErrorAction SilentlyContinue
    } else {
        $env:AGENTERM_WORKSPACE_PATH = $previousWorkspace
    }
    if ($null -eq $previousSettings) {
        Remove-Item Env:AGENTERM_SETTINGS_PATH -ErrorAction SilentlyContinue
    } else {
        $env:AGENTERM_SETTINGS_PATH = $previousSettings
    }
}
