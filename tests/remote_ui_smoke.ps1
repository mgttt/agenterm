param(
    [string]$GuiExe = (Join-Path $PSScriptRoot '..\dist\agenterm.exe'),
    [string]$CliExe = (Join-Path $PSScriptRoot '..\dist\agenterm-cli.exe'),
    [string]$ScreenshotPath = '',
    [switch]$ListEvidence
)

$ErrorActionPreference = 'Stop'
$declaredEvidence = @('ui.replaceable-client')
if ($ListEvidence) {
    $declaredEvidence
    exit 0
}

. (Join-Path $PSScriptRoot 'TestHarness.ps1')

Add-Type -AssemblyName System.Drawing
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class AgenTermRemoteUiNativeTest {
    [StructLayout(LayoutKind.Sequential)]
    public struct Rect {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [DllImport("user32.dll")]
    public static extern bool GetWindowRect(IntPtr window, out Rect rect);

    [DllImport("user32.dll")]
    public static extern bool PrintWindow(
        IntPtr window, IntPtr device, uint flags);
}
'@

function Save-WindowPng {
    param(
        [Parameter(Mandatory = $true)][IntPtr]$Window,
        [Parameter(Mandatory = $true)][string]$Path
    )
    $rect = [AgenTermRemoteUiNativeTest+Rect]::new()
    if (-not [AgenTermRemoteUiNativeTest]::GetWindowRect(
            $Window, [ref]$rect
        )) {
        throw 'GetWindowRect failed for replaceable UI'
    }
    $width = $rect.Right - $rect.Left
    $height = $rect.Bottom - $rect.Top
    if ($width -lt 320 -or $height -lt 240) {
        throw "replaceable UI window bounds are invalid: ${width}x${height}"
    }
    $bitmap = [Drawing.Bitmap]::new($width, $height)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    $device = $graphics.GetHdc()
    try {
        if (-not [AgenTermRemoteUiNativeTest]::PrintWindow(
                $Window, $device, 2
            )) {
            throw 'PrintWindow failed for replaceable UI'
        }
    }
    finally {
        $graphics.ReleaseHdc($device)
        $graphics.Dispose()
    }
    try {
        $bitmap.Save($Path, [Drawing.Imaging.ImageFormat]::Png)
    }
    finally {
        $bitmap.Dispose()
    }
    if ((Get-Item -LiteralPath $Path).Length -lt 1000) {
        throw 'replaceable UI PNG evidence is unexpectedly small'
    }
}

$GuiExe = [IO.Path]::GetFullPath($GuiExe)
$run = New-SmokeRunContext -Suite 'remote-ui' -Executable $CliExe `
    -DeclaredEvidence $declaredEvidence -AllowPaneCapture
$CliExe = $run.Executable
$gui = $null
$runSucceeded = $false
$runFailure = $null

function Invoke-AgenTerm {
    param([string[]]$CommandArgs)
    Invoke-SmokeCli -Context $run -Arguments $CommandArgs
}

function Write-Evidence {
    param([Parameter(Mandatory = $true)][string]$Id)
    Write-SmokeEvidence -Context $run -Id $Id
}

try {
    Write-Host 'STEP start replaceable UI and let it start the independent server'
    $stderrPath = Join-Path $run.RunDirectory 'remote-ui-stderr.txt'
    $gui = Start-Process -FilePath $GuiExe `
        -ArgumentList @(
            '--ui-client', '--no-activate', '--address', $run.Address
        ) `
        -RedirectStandardError $stderrPath `
        -PassThru -WindowStyle Normal
    Register-SmokeOwnedProcess -Context $run -Id $gui.Id `
        -Kind 'gui' -Address $run.Address

    $ready = $false
    $deadline = [DateTime]::UtcNow.AddSeconds(12)
    do {
        Start-Sleep -Milliseconds 50
        $gui.Refresh()
        $leaseOutput = @(
            & $CliExe --address $run.Address ui-lease status 2>&1
        )
        if ($LASTEXITCODE -eq 0 -and $gui.MainWindowHandle -ne 0) {
            $lease = ($leaseOutput -join "`n") | ConvertFrom-Json
            if ($lease.attached -and $lease.client_pid -eq $gui.Id) {
                $ready = $true
                break
            }
        }
    } while ([DateTime]::UtcNow -lt $deadline)
    if (-not $ready) {
        $stderr = Get-Content -LiteralPath $stderrPath -Raw `
            -ErrorAction SilentlyContinue
        throw (
            "replaceable UI did not become ready: pid=$($gui.Id) " +
            "hwnd=$($gui.MainWindowHandle)`n$stderr"
        )
    }
    Sync-SmokeOwnedServers -Context $run

    Write-Host 'STEP prove GUI and server are different process roles'
    $bootstrap = Invoke-AgenTerm @('ui-bootstrap') | ConvertFrom-Json
    $protocol = Invoke-AgenTerm @('protocol-info', '--running') |
        ConvertFrom-Json
    $guiHandle = [int64]$gui.MainWindowHandle
    if ($bootstrap.schema_version -ne 2 -or
        $bootstrap.server_pid -eq $gui.Id -or
        $bootstrap.tabs.Count -lt 1 -or
        $protocol.ui_bridge.ownership_mode -ne 'split_server_client' -or
        -not $protocol.ui_bridge.interactive_lease -or
        $guiHandle -eq 0) {
        throw 'replaceable GUI and headless authority roles were not separated'
    }
    Write-Host 'STEP prove the server-owned PTY remains interactive'
    $tabId = [string]$bootstrap.active_tab_id
    $marker = "AGENTERM_REMOTE_UI_$($run.RunId)"
    Invoke-AgenTerm @(
        'send-keys', '-t', $tabId, '-l', "echo $marker"
    ) | Out-Null
    Invoke-AgenTerm @('send-keys', '-t', $tabId, 'Enter') | Out-Null
    Invoke-AgenTerm @(
        'wait-pane', '-t', $tabId,
        '--contains', $marker, '--timeout-ms', '5000'
    ) | Out-Null
    $renderedPosition = (
        Invoke-AgenTerm @('ui-bootstrap') | ConvertFrom-Json
    ).position.sequence
    $observed = $false
    $observeDeadline = [DateTime]::UtcNow.AddSeconds(5)
    do {
        $lease = Invoke-AgenTerm @('ui-lease', 'status') |
            ConvertFrom-Json
        if ($lease.observed_sequence -ge $renderedPosition) {
            $observed = $true
            break
        }
        Start-Sleep -Milliseconds 25
    } while ([DateTime]::UtcNow -lt $observeDeadline)
    if (-not $observed) {
        throw 'replaceable UI did not acknowledge the rendered Fleet position'
    }
    Write-Host 'STEP close only the GUI and retain server, PTY, and workspace'
    if (-not $gui.CloseMainWindow()) {
        throw 'replaceable UI did not expose a closable HWND'
    }
    if (-not $gui.WaitForExit(5000)) {
        throw 'replaceable UI did not exit after bounded WM_CLOSE'
    }
    $leaseAfter = Invoke-AgenTerm @('ui-lease', 'status') |
        ConvertFrom-Json
    $captureAfter = Invoke-AgenTerm @(
        'capture-pane', '-p', '-t', $tabId, '--max-bytes', '16384'
    )
    if ($leaseAfter.attached -or -not $captureAfter.Contains($marker)) {
        throw 'closing replaceable UI incorrectly ended its server or PTY'
    }

    Write-Host 'STEP replace the GUI and recover the same live server state'
    $replacementStderr = Join-Path $run.RunDirectory `
        'remote-ui-replacement-stderr.txt'
    $replacement = Start-Process -FilePath $GuiExe `
        -ArgumentList @(
            '--ui-client', '--no-activate', '--address', $run.Address
        ) `
        -RedirectStandardError $replacementStderr `
        -PassThru -WindowStyle Normal
    Register-SmokeOwnedProcess -Context $run -Id $replacement.Id `
        -Kind 'gui' -Address $run.Address
    $gui = $replacement
    $replacementReady = $false
    $replacementDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        Start-Sleep -Milliseconds 50
        $gui.Refresh()
        $replacementLease = Invoke-AgenTerm @('ui-lease', 'status') |
            ConvertFrom-Json
        if ($gui.MainWindowHandle -ne 0 -and
            $replacementLease.attached -and
            $replacementLease.client_pid -eq $gui.Id) {
            $replacementReady = $true
            break
        }
    } while ([DateTime]::UtcNow -lt $replacementDeadline)
    if (-not $replacementReady) {
        throw 'replacement GUI did not acquire the released UI lease'
    }
    $replacementBootstrap = Invoke-AgenTerm @('ui-bootstrap') |
        ConvertFrom-Json
    $replacementCapture = Invoke-AgenTerm @(
        'capture-pane', '-p', '-t', $tabId, '--max-bytes', '16384'
    )
    $replacementObserved = $false
    $replacementObserveDeadline = [DateTime]::UtcNow.AddSeconds(5)
    do {
        $replacementLease = Invoke-AgenTerm @('ui-lease', 'status') |
            ConvertFrom-Json
        if ($replacementLease.observed_sequence -ge
            $replacementBootstrap.position.sequence) {
            $replacementObserved = $true
            break
        }
        Start-Sleep -Milliseconds 25
    } while ([DateTime]::UtcNow -lt $replacementObserveDeadline)
    if ($replacementBootstrap.server_pid -ne $bootstrap.server_pid -or
        $replacementBootstrap.active_tab_id -ne $tabId -or
        -not $replacementCapture.Contains($marker) -or
        -not $replacementObserved) {
        throw (
            'replacement GUI did not recover the same server and terminal ' +
            "state: expected_server=$($bootstrap.server_pid) " +
            "actual_server=$($replacementBootstrap.server_pid) " +
            "expected_tab=$tabId " +
            "actual_tab=$($replacementBootstrap.active_tab_id) " +
            "marker_present=$($replacementCapture.Contains($marker)) " +
            "observed=$($replacementLease.observed_sequence) " +
            "position=$($replacementBootstrap.position.sequence)"
        )
    }
    if ([string]::IsNullOrWhiteSpace($ScreenshotPath)) {
        $ScreenshotPath = Join-Path $run.RunDirectory 'remote-ui.png'
    } else {
        $ScreenshotPath = [IO.Path]::GetFullPath($ScreenshotPath)
    }
    Save-WindowPng -Window $gui.MainWindowHandle -Path $ScreenshotPath
    if (-not $gui.CloseMainWindow() -or -not $gui.WaitForExit(5000)) {
        throw 'replacement GUI did not release its lease through bounded close'
    }
    $replacementLeaseAfter = Invoke-AgenTerm @('ui-lease', 'status') |
        ConvertFrom-Json
    if ($replacementLeaseAfter.attached) {
        throw 'replacement GUI left its interactive lease attached'
    }

    Write-Evidence 'ui.replaceable-client'
    Invoke-AgenTerm @('shutdown') | Out-Null
    $runSucceeded = $true
}
catch {
    $runFailure = $_
}
finally {
    Complete-SmokeRun -Context $run -Succeeded $runSucceeded `
        -FailureRecord $runFailure
}

if (-not $runSucceeded) {
    throw $runFailure
}
Write-Host 'PASS: replaceable GUI attaches, renders, detaches, and leaves server PTYs alive'
