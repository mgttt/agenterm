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
    public static extern bool GetClientRect(IntPtr window, out Rect rect);

    [DllImport("user32.dll")]
    public static extern bool PrintWindow(
        IntPtr window, IntPtr device, uint flags);

    [DllImport("user32.dll")]
    public static extern IntPtr SendMessage(
        IntPtr window, uint message, IntPtr wparam, IntPtr lparam);

    [DllImport("user32.dll", EntryPoint = "SendMessageW",
        CharSet = CharSet.Unicode)]
    public static extern IntPtr SendMessageText(
        IntPtr window, uint message, IntPtr wparam, string text);

    [DllImport("user32.dll")]
    public static extern IntPtr GetDlgItem(IntPtr window, int id);

    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr window);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetWindowText(
        IntPtr window, System.Text.StringBuilder text, int maxCount);

    [DllImport("user32.dll")]
    public static extern bool SetWindowPos(
        IntPtr window, IntPtr insertAfter, int x, int y, int width, int height,
        uint flags);

    public static IntPtr MousePoint(int x, int y) {
        return new IntPtr((y << 16) | (x & 0xffff));
    }

    public static IntPtr WheelDelta(short delta) {
        return new IntPtr((long)(ushort)delta << 16);
    }
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

function Get-NativeText {
    param([Parameter(Mandatory = $true)][IntPtr]$Control)
    $text = [Text.StringBuilder]::new(128)
    [AgenTermRemoteUiNativeTest]::GetWindowText(
        $Control, $text, $text.Capacity
    ) | Out-Null
    $text.ToString()
}

$GuiExe = [IO.Path]::GetFullPath($GuiExe)
$ServerExe = Join-Path ([IO.Path]::GetDirectoryName($GuiExe)) `
    'agenterm-server.exe'
if (-not (Test-Path -LiteralPath $ServerExe)) {
    throw "AgenTerm server executable not found: $ServerExe"
}
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

function Wait-UiProjection {
    param(
        [Parameter(Mandatory = $true)][string]$Projection,
        [int]$TimeoutMs = 5000
    )
    $deadline = [DateTime]::UtcNow.AddMilliseconds($TimeoutMs)
    do {
        $snapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
        if ($snapshot.projection -eq $Projection) {
            return $snapshot
        }
        Start-Sleep -Milliseconds 25
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "UI projection did not become '$Projection' within ${TimeoutMs}ms"
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

    Write-Host 'STEP observe the lease-owned GUI projection through public CLI'
    $uiSnapshot = Wait-UiProjection -Projection 'replaceable_ui_client'
    if ($uiSnapshot.schema_version -ne 1 -or
        $uiSnapshot.client_pid -ne $gui.Id -or
        $uiSnapshot.event_position.epoch -ne
            $lease.position.server_epoch -or
        $uiSnapshot.window.visible -ne $true -or
        $uiSnapshot.window.client_width -lt 320 -or
        $uiSnapshot.layout.terminal.rows -lt 1 -or
        $uiSnapshot.layout.composer.input.target_rows -ne 3 -or
        $uiSnapshot.layout.status_bar.proxy.archived -ne $true -or
        $uiSnapshot.focus.surface -notin @('terminal', 'composer', 'tabs') -or
        $uiSnapshot.tabs.Count -lt 1) {
        throw 'public UI snapshot did not expose the replaceable GUI projection'
    }

    Write-Host 'STEP relay typed UI actions to the exact live GUI lease'
    $hiddenSnapshot = Invoke-AgenTerm @('ui-action', 'tabs-hide') |
        ConvertFrom-Json
    if ($hiddenSnapshot.projection -ne 'replaceable_ui_client' -or
        $hiddenSnapshot.client_pid -ne $gui.Id -or
        $hiddenSnapshot.layout.sidebar.visible -ne $false -or
        $hiddenSnapshot.layout.status_bar.tabs_recovery.width -le 0) {
        throw 'relayed Tabs hide did not return the completed GUI snapshot'
    }
    $shownSnapshot = Invoke-AgenTerm @('ui-action', 'tabs-show') |
        ConvertFrom-Json
    if ($shownSnapshot.layout.sidebar.visible -ne $true -or
        $shownSnapshot.client_pid -ne $gui.Id) {
        throw 'relayed Tabs show did not execute in the lease-owned GUI'
    }
    $settingsSnapshot = Invoke-AgenTerm @('ui-action', 'open-settings') |
        ConvertFrom-Json
    if ($settingsSnapshot.modal.kind -ne 'settings') {
        throw 'relayed Settings action did not expose its client-owned modal'
    }
    $cancelledSnapshot = Invoke-AgenTerm @('ui-action', 'cancel') |
        ConvertFrom-Json
    if ($null -ne $cancelledSnapshot.modal) {
        throw 'relayed Cancel did not close the client-owned Settings modal'
    }
    $composerSnapshot = Invoke-AgenTerm @('focus', 'composer') |
        ConvertFrom-Json
    if ($composerSnapshot.focus.surface -ne 'composer') {
        throw 'relayed focus did not reach the native Composer'
    }
    Invoke-AgenTerm @('focus', 'terminal') | Out-Null
    $relayedSettings = Invoke-AgenTerm @('get-settings') | ConvertFrom-Json
    if ($relayedSettings.terminal_font_size -lt 8 -or
        $relayedSettings.recommended_cjk_font -ne 'Sarasa Fixed SC') {
        throw 'relayed Settings discovery did not come from the GUI client'
    }
    $relayedPng = Join-Path $run.RunDirectory 'relayed-window.png'
    $returnedPng = (Invoke-AgenTerm @(
        'screenshot', '-o', $relayedPng
    )).Trim()
    if ([IO.Path]::GetFullPath($returnedPng) -ne
            [IO.Path]::GetFullPath($relayedPng) -or
        (Get-Item -LiteralPath $relayedPng).Length -lt 1000) {
        throw 'relayed screenshot did not return a valid GUI-owned PNG'
    }

    Write-Host 'STEP prove GUI and server are different process roles'
    $bootstrap = Invoke-AgenTerm @('ui-bootstrap') | ConvertFrom-Json
    $protocol = Invoke-AgenTerm @('protocol-info', '--running') |
        ConvertFrom-Json
    $guiHandle = [int64]$gui.MainWindowHandle
    if ($bootstrap.schema_version -ne 2 -or
        $uiSnapshot.server_pid -ne $bootstrap.server_pid -or
        $bootstrap.server_pid -eq $gui.Id -or
        $bootstrap.tabs.Count -lt 1 -or
        $lease.client_build.protocol_version -ne 1 -or
        [string]::IsNullOrWhiteSpace($lease.client_build.version) -or
        $protocol.ui_bridge.ownership_mode -ne 'split_server_client' -or
        -not $protocol.ui_bridge.replaceable_ui -or
        -not $protocol.ui_bridge.interactive_lease -or
        -not $protocol.ui_bridge.reconnect -or
        -not $protocol.ui_bridge.rollback_proven -or
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
    $composerEditor = [AgenTermRemoteUiNativeTest]::GetDlgItem(
        $gui.MainWindowHandle, 2101
    )
    $closeKeep = [AgenTermRemoteUiNativeTest]::GetDlgItem(
        $gui.MainWindowHandle, 2109
    )
    $closeStop = [AgenTermRemoteUiNativeTest]::GetDlgItem(
        $gui.MainWindowHandle, 2110
    )
    $closeCancel = [AgenTermRemoteUiNativeTest]::GetDlgItem(
        $gui.MainWindowHandle, 2111
    )
    $draftMarker = "draft-$($run.RunId)"
    [AgenTermRemoteUiNativeTest]::SendMessageText(
        $composerEditor, 0x000C, [IntPtr]::Zero, $draftMarker
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    if ($gui.HasExited -or
        -not [AgenTermRemoteUiNativeTest]::IsWindowVisible($closeKeep) -or
        -not [AgenTermRemoteUiNativeTest]::IsWindowVisible($closeStop) -or
        -not [AgenTermRemoteUiNativeTest]::IsWindowVisible($closeCancel) -or
        [AgenTermRemoteUiNativeTest]::IsWindowVisible($composerEditor)) {
        throw 'WM_CLOSE did not open the non-blocking three-choice close surface'
    }
    Save-WindowPng -Window $gui.MainWindowHandle `
        -Path (Join-Path $run.RunDirectory 'close-three-choice.png')
    if (-not [string]::IsNullOrWhiteSpace($ScreenshotPath)) {
        $requestedScreenshot = [IO.Path]::GetFullPath($ScreenshotPath)
        $closeScreenshot = Join-Path `
            ([IO.Path]::GetDirectoryName($requestedScreenshot)) `
            "$([IO.Path]::GetFileNameWithoutExtension($requestedScreenshot))-close.png"
        Save-WindowPng -Window $gui.MainWindowHandle -Path $closeScreenshot
    }
    $savedDraft = Invoke-AgenTerm @('show-composer', '-t', $tabId)
    if (($savedDraft -join "`n").TrimEnd() -ne $draftMarker) {
        throw 'window-close surface did not preserve the current Composer draft'
    }
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $closeCancel, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    if ($gui.HasExited -or
        [AgenTermRemoteUiNativeTest]::IsWindowVisible($closeKeep) -or
        -not [AgenTermRemoteUiNativeTest]::IsWindowVisible($composerEditor)) {
        throw 'Cancel did not return from the close surface without exiting'
    }
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0100, [IntPtr]0x1B, [IntPtr]::Zero
    ) | Out-Null
    if ($gui.HasExited -or
        [AgenTermRemoteUiNativeTest]::IsWindowVisible($closeKeep)) {
        throw 'Esc did not perform the same non-mutating close cancellation'
    }
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0100, [IntPtr]0x0D, [IntPtr]::Zero
    ) | Out-Null
    if (-not $gui.WaitForExit(5000)) {
        throw 'Enter did not invoke the default Keep Server Running choice'
    }
    $leaseAfter = Invoke-AgenTerm @('ui-lease', 'status') |
        ConvertFrom-Json
    $captureAfter = Invoke-AgenTerm @(
        'capture-pane', '-p', '-t', $tabId, '--max-bytes', '16384'
    )
    if ($leaseAfter.attached -or -not $captureAfter.Contains($marker)) {
        throw 'closing replaceable UI incorrectly ended its server or PTY'
    }
    $headlessSnapshot = Wait-UiProjection -Projection 'headless_server'
    if ($headlessSnapshot.server_pid -ne $bootstrap.server_pid -or
        $headlessSnapshot.tabs.Count -lt 1) {
        throw 'public UI snapshot did not fall back to the retained server'
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
        $replacementLease.client_build.protocol_version -ne 1 -or
        [string]::IsNullOrWhiteSpace(
            $replacementLease.client_build.version
        ) -or
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

    Write-Host 'STEP recover hidden Tabs from the persistent toolbar, system menu, and status bar'
    $tabsButton = [AgenTermRemoteUiNativeTest]::GetDlgItem(
        $gui.MainWindowHandle, 2104
    )
    $settingsButton = [AgenTermRemoteUiNativeTest]::GetDlgItem(
        $gui.MainWindowHandle, 2112
    )
    $newButton = [AgenTermRemoteUiNativeTest]::GetDlgItem(
        $gui.MainWindowHandle, 2103
    )
    $toolbarUi = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    if ((Get-NativeText $tabsButton) -ne '<Tabs' -or
        $toolbarUi.layout.toolbar.tabs.left -ge
            $toolbarUi.layout.toolbar.new.left -or
        $toolbarUi.layout.toolbar.settings.right -ne
            ($toolbarUi.layout.toolbar.bounds.right - 8)) {
        throw 'toolbar order, visible Tabs direction, or Settings alignment is wrong'
    }
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $tabsButton, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    if (-not [AgenTermRemoteUiNativeTest]::IsWindowVisible($tabsButton) -or
        -not [AgenTermRemoteUiNativeTest]::IsWindowVisible($settingsButton) -or
        (Get-NativeText $tabsButton) -ne '>Tabs') {
        throw 'hiding Tabs also hid the terminal workbench toolbar'
    }
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $tabsButton, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    if (-not [AgenTermRemoteUiNativeTest]::IsWindowVisible($tabsButton)) {
        throw 'persistent toolbar Tabs control did not restore the sidebar'
    }
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0112, [IntPtr]0x1F20, [IntPtr]::Zero
    ) | Out-Null
    if (-not [AgenTermRemoteUiNativeTest]::IsWindowVisible($tabsButton)) {
        throw 'system-menu Toggle Tabs incorrectly hid the terminal toolbar'
    }
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0112, [IntPtr]0x1F20, [IntPtr]::Zero
    ) | Out-Null
    if (-not [AgenTermRemoteUiNativeTest]::IsWindowVisible($tabsButton)) {
        throw 'system-menu Toggle Tabs did not restore the sidebar'
    }

    [AgenTermRemoteUiNativeTest]::SendMessage(
        $tabsButton, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    $hiddenTabsClient = [AgenTermRemoteUiNativeTest+Rect]::new()
    if (-not [AgenTermRemoteUiNativeTest]::GetClientRect(
            $gui.MainWindowHandle, [ref]$hiddenTabsClient
        )) {
        throw 'GetClientRect failed for hidden Tabs recovery'
    }
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0201, [IntPtr]::Zero,
        [AgenTermRemoteUiNativeTest]::MousePoint(
            36, $hiddenTabsClient.Bottom - 13
        )
    ) | Out-Null
    if (-not [AgenTermRemoteUiNativeTest]::IsWindowVisible($tabsButton)) {
        throw 'bottom status Tabs recovery segment did not restore the sidebar'
    }

    Write-Host 'STEP configure a new terminal before creation'
    $newInitialCommand = [AgenTermRemoteUiNativeTest]::GetDlgItem(
        $gui.MainWindowHandle, 2124
    )
    $newHttpProxy = [AgenTermRemoteUiNativeTest]::GetDlgItem(
        $gui.MainWindowHandle, 2125
    )
    $newHttpsProxy = [AgenTermRemoteUiNativeTest]::GetDlgItem(
        $gui.MainWindowHandle, 2126
    )
    $newCreate = [AgenTermRemoteUiNativeTest]::GetDlgItem(
        $gui.MainWindowHandle, 2127
    )
    $beforeNewTabs = @((Invoke-AgenTerm @('ui-bootstrap') |
            ConvertFrom-Json).tabs).Count
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $newButton, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    Invoke-AgenTerm @(
        'wait-ui', '--modal-kind', 'new-terminal', '--timeout-ms', '1000'
    ) | Out-Null
    if (@((Invoke-AgenTerm @('ui-bootstrap') |
                ConvertFrom-Json).tabs).Count -ne $beforeNewTabs -or
        -not [AgenTermRemoteUiNativeTest]::IsWindowVisible($newInitialCommand)) {
        throw 'New mutated the tab set before Create or did not open its modal'
    }
    $newMarker = "AGENTERM_NEW_DIALOG_$($run.RunId)"
    $proxySecret = 'http://dialog-user:dialog-pass@127.0.0.1:48888'
    [AgenTermRemoteUiNativeTest]::SendMessageText(
        $newInitialCommand, 0x000C, [IntPtr]::Zero,
        "if defined HTTP_PROXY echo $newMarker"
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessageText(
        $newHttpProxy, 0x000C, [IntPtr]::Zero, $proxySecret
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessageText(
        $newHttpsProxy, 0x000C, [IntPtr]::Zero, $proxySecret
    ) | Out-Null
    $modalDeadline = [DateTime]::UtcNow.AddSeconds(2)
    do {
        $newModalText = Invoke-AgenTerm @('ui-snapshot')
        $newModal = $newModalText | ConvertFrom-Json
        if ($newModal.modal.initial_command_configured -and
            $newModal.modal.http_proxy_configured -and
            $newModal.modal.https_proxy_configured) {
            break
        }
    } while ([DateTime]::UtcNow -lt $modalDeadline)
    if (-not $newModal.modal.initial_command_configured -or
        -not $newModal.modal.http_proxy_configured -or
        -not $newModal.modal.https_proxy_configured -or
        $newModal.modal.proxy_values_exposed -or
        $newModalText.Contains($proxySecret) -or
        $newModalText.Contains('dialog-pass')) {
        throw 'New terminal modal did not expose redacted configured-state facts'
    }
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $newCreate, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    Invoke-AgenTerm @(
        'wait-pane', '--contains', $newMarker, '--timeout-ms', '5000'
    ) | Out-Null
    if (@((Invoke-AgenTerm @('ui-bootstrap') |
                ConvertFrom-Json).tabs).Count -ne ($beforeNewTabs + 1)) {
        throw 'Create did not add exactly one configured terminal'
    }
    $createdTabId = (Invoke-AgenTerm @('ui-bootstrap') |
            ConvertFrom-Json).active_tab_id
    Invoke-AgenTerm @('select-window', '-t', $tabId) | Out-Null
    Invoke-AgenTerm @('kill-window', '-t', $createdTabId) | Out-Null
    Invoke-AgenTerm @(
        'wait-ui', '--active', $tabId, '--focus', 'terminal',
        '--timeout-ms', '1000'
    ) | Out-Null

    Write-Host 'STEP navigate Terminal, Composer, and Tabs without the mouse'
    $focusMessage = 0x8003
    $shortcutMessage = 0x8002
    if ([AgenTermRemoteUiNativeTest]::SendMessage(
            $gui.MainWindowHandle, $focusMessage,
            [IntPtr]::Zero, [IntPtr]::Zero
        ).ToInt64() -ne 1) {
        throw 'replaceable UI did not start with terminal focus'
    }
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, $shortcutMessage,
        [IntPtr]0x28, [IntPtr]1
    ) | Out-Null
    if ([AgenTermRemoteUiNativeTest]::SendMessage(
            $gui.MainWindowHandle, $focusMessage,
            [IntPtr]::Zero, [IntPtr]::Zero
        ).ToInt64() -ne 2) {
        throw 'Ctrl+Down did not move focus from terminal to Composer'
    }
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, $shortcutMessage,
        [IntPtr]0x26, [IntPtr]1
    ) | Out-Null
    if ([AgenTermRemoteUiNativeTest]::SendMessage(
            $gui.MainWindowHandle, $focusMessage,
            [IntPtr]::Zero, [IntPtr]::Zero
        ).ToInt64() -ne 1) {
        throw 'Ctrl+Up did not return focus from Composer to terminal'
    }
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, $shortcutMessage,
        [IntPtr]0x25, [IntPtr]1
    ) | Out-Null
    if ([AgenTermRemoteUiNativeTest]::SendMessage(
            $gui.MainWindowHandle, $focusMessage,
            [IntPtr]::Zero, [IntPtr]::Zero
        ).ToInt64() -ne 3) {
        throw 'Ctrl+Left did not move focus from terminal to Tabs'
    }
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, $shortcutMessage,
        [IntPtr]0x27, [IntPtr]1
    ) | Out-Null
    if ([AgenTermRemoteUiNativeTest]::SendMessage(
            $gui.MainWindowHandle, $focusMessage,
            [IntPtr]::Zero, [IntPtr]::Zero
        ).ToInt64() -ne 1) {
        throw 'Ctrl+Right did not return focus from Tabs to terminal'
    }
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, $shortcutMessage,
        [IntPtr]0x28, [IntPtr]3
    ) | Out-Null
    if ([AgenTermRemoteUiNativeTest]::SendMessage(
            $gui.MainWindowHandle, $focusMessage,
            [IntPtr]::Zero, [IntPtr]::Zero
        ).ToInt64() -ne 1) {
        throw 'Ctrl+Shift+Down incorrectly stole the native key behavior'
    }

    Write-Host 'STEP preview, cancel, and apply client-owned Settings'
    $replacementComposer = [AgenTermRemoteUiNativeTest]::GetDlgItem(
        $gui.MainWindowHandle, 2101
    )
    $settingsFont = [AgenTermRemoteUiNativeTest]::GetDlgItem(
        $gui.MainWindowHandle, 2113
    )
    $settingsSize = [AgenTermRemoteUiNativeTest]::GetDlgItem(
        $gui.MainWindowHandle, 2114
    )
    $settingsDark = [AgenTermRemoteUiNativeTest]::GetDlgItem(
        $gui.MainWindowHandle, 2115
    )
    $settingsLight = [AgenTermRemoteUiNativeTest]::GetDlgItem(
        $gui.MainWindowHandle, 2116
    )
    $settingsApply = [AgenTermRemoteUiNativeTest]::GetDlgItem(
        $gui.MainWindowHandle, 2117
    )
    $settingsCancel = [AgenTermRemoteUiNativeTest]::GetDlgItem(
        $gui.MainWindowHandle, 2118
    )
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $settingsButton, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    if (-not [AgenTermRemoteUiNativeTest]::IsWindowVisible($settingsFont) -or
        -not [AgenTermRemoteUiNativeTest]::IsWindowVisible($settingsLight) -or
        -not [AgenTermRemoteUiNativeTest]::IsWindowVisible($settingsApply) -or
        [AgenTermRemoteUiNativeTest]::IsWindowVisible($replacementComposer)) {
        throw 'Settings did not replace the workbench controls with its native modal'
    }
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $settingsLight, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    if (-not [string]::IsNullOrWhiteSpace($ScreenshotPath)) {
        $requestedScreenshot = [IO.Path]::GetFullPath($ScreenshotPath)
        $settingsScreenshot = Join-Path `
            ([IO.Path]::GetDirectoryName($requestedScreenshot)) `
            "$([IO.Path]::GetFileNameWithoutExtension($requestedScreenshot))-settings.png"
        Save-WindowPng -Window $gui.MainWindowHandle -Path $settingsScreenshot
    }
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $settingsCancel, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    if ([AgenTermRemoteUiNativeTest]::IsWindowVisible($settingsFont) -or
        -not [AgenTermRemoteUiNativeTest]::IsWindowVisible($replacementComposer)) {
        throw 'Settings Cancel did not restore the workbench'
    }
    if (Test-Path -LiteralPath $run.SettingsPath) {
        $cancelledSettings = Get-Content -LiteralPath $run.SettingsPath -Raw |
            ConvertFrom-Json
        if ($cancelledSettings.color_theme -eq 'light') {
            throw 'Settings Cancel persisted the preview theme'
        }
    }

    [AgenTermRemoteUiNativeTest]::SendMessage(
        $settingsButton, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessageText(
        $settingsFont, 0x000C, [IntPtr]::Zero, 'Consolas'
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessageText(
        $settingsSize, 0x000C, [IntPtr]::Zero, '14'
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $settingsLight, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $settingsApply, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    $appliedSettings = Get-Content -LiteralPath $run.SettingsPath -Raw |
        ConvertFrom-Json
    if ($appliedSettings.color_theme -ne 'light' -or
        $appliedSettings.terminal_font_family -ne 'Consolas' -or
        $appliedSettings.terminal_font_size -ne 14 -or
        [AgenTermRemoteUiNativeTest]::IsWindowVisible($settingsFont) -or
        -not [AgenTermRemoteUiNativeTest]::IsWindowVisible($replacementComposer)) {
        throw 'Settings Apply did not persist theme/font and restore the workbench'
    }

    [AgenTermRemoteUiNativeTest]::SendMessage(
        $settingsButton, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $settingsDark, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $settingsCancel, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    $cancelledDark = Get-Content -LiteralPath $run.SettingsPath -Raw |
        ConvertFrom-Json
    if ($cancelledDark.color_theme -ne 'light') {
        throw 'Settings Cancel did not roll the preview back to the applied theme'
    }

    Write-Host 'STEP edit CWD through the bottom workbench segment'
    $cwdSnapshot = Wait-UiProjection -Projection 'replaceable_ui_client'
    $cwdBounds = $cwdSnapshot.layout.status_bar.cwd.bounds
    $cwdWidth = [int]$cwdBounds.right - [int]$cwdBounds.left
    $cwdHeight = [int]$cwdBounds.bottom - [int]$cwdBounds.top
    if ($cwdWidth -le 0 -or $cwdHeight -le 0) {
        throw 'ui-snapshot did not expose an actionable CWD status segment'
    }
    $cwdStatusX = [int]$cwdBounds.left + [int]($cwdWidth / 2)
    $cwdStatusY = [int]$cwdBounds.top + [int]($cwdHeight / 2)
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0201, [IntPtr]::Zero,
        [AgenTermRemoteUiNativeTest]::MousePoint(
            $cwdStatusX, $cwdStatusY
        )
    ) | Out-Null
    $cwdPath = Join-Path $run.RunDirectory 'cwd target'
    [AgenTermRemoteUiNativeTest]::SendMessageText(
        $replacementComposer, 0x000C, [IntPtr]::Zero, $cwdPath
    ) | Out-Null
    $replacementSend = [AgenTermRemoteUiNativeTest]::GetDlgItem(
        $gui.MainWindowHandle, 2102
    )
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $replacementSend, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    $cwdPrepared = Invoke-AgenTerm @('ui-bootstrap') |
        ConvertFrom-Json
    $cwdTab = @(
        $cwdPrepared.tabs | Where-Object id -eq $tabId
    )[0]
    $cwdComposer = (
        Invoke-AgenTerm @('show-composer', '-t', $tabId) -join "`n"
    ).TrimEnd()
    if ($cwdTab.working_context.cwd -ne $cwdPath -or
        -not $cwdTab.working_context.cwd_request_pending -or
        $cwdComposer -notlike "*$cwdPath*") {
        throw (
            'CWD Prepare did not publish pending context and a shell-safe Composer draft: ' +
            ($cwdTab | ConvertTo-Json -Depth 8 -Compress)
        )
    }

    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0201, [IntPtr]::Zero,
        [AgenTermRemoteUiNativeTest]::MousePoint(
            $cwdStatusX, $cwdStatusY
        )
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessageText(
        $replacementComposer, 0x000C, [IntPtr]::Zero, 'must-not-prepare'
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0201, [IntPtr]::Zero,
        [AgenTermRemoteUiNativeTest]::MousePoint(
            $cwdStatusX, $cwdStatusY
        )
    ) | Out-Null
    $cwdAfterCancel = (
        Invoke-AgenTerm @('show-composer', '-t', $tabId) -join "`n"
    ).TrimEnd()
    if ($cwdAfterCancel -ne $cwdComposer) {
        throw 'CWD editor Cancel changed the prepared Composer draft'
    }

    Write-Host 'STEP resize Tabs locally and scroll the server-owned viewport'
    $activeBeforeResize = @(
        $replacementBootstrap.tabs |
            Where-Object id -eq $replacementBootstrap.active_tab_id
    )[0]
    $columnsBeforeResize = [int]$activeBeforeResize.screen.columns
    $resizeY = 200
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0201, [IntPtr]::Zero,
        [AgenTermRemoteUiNativeTest]::MousePoint(249, $resizeY)
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0200, [IntPtr]::Zero,
        [AgenTermRemoteUiNativeTest]::MousePoint(360, $resizeY)
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0202, [IntPtr]::Zero,
        [AgenTermRemoteUiNativeTest]::MousePoint(360, $resizeY)
    ) | Out-Null
    $savedSettings = Get-Content -LiteralPath $run.SettingsPath -Raw |
        ConvertFrom-Json
    $resizedBootstrap = Invoke-AgenTerm @('ui-bootstrap') |
        ConvertFrom-Json
    $activeAfterResize = @(
        $resizedBootstrap.tabs |
            Where-Object id -eq $resizedBootstrap.active_tab_id
    )[0]
    if ($savedSettings.tabs_width -ne 360 -or
        $activeAfterResize.screen.columns -ge $columnsBeforeResize) {
        throw (
            'replaceable UI did not persist Tabs drag or resize the PTY: ' +
            "saved_width=$($savedSettings.tabs_width) " +
            "before_columns=$columnsBeforeResize " +
            "after_columns=$($activeAfterResize.screen.columns)"
        )
    }

    Write-Host 'STEP add and close a child through the typed replaceable Tabs tree'
    $rootUi = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $addBaseline = $rootUi.event_position
    Invoke-AgenTerm @('ui-action', 'new-child', '-t', $tabId) | Out-Null
    $createdEvent = Invoke-AgenTerm @(
        'wait-events',
        '--epoch', $addBaseline.epoch,
        '--after', "$($addBaseline.sequence)",
        '--kind', 'tab.created',
        '--timeout-ms', '5000'
    ) | ConvertFrom-Json
    $childId = "@$($createdEvent.tab_id)"
    Invoke-AgenTerm @(
        'wait-ui', '--active', $childId, '--timeout-ms', '5000'
    ) | Out-Null
    $childBootstrap = Invoke-AgenTerm @('ui-bootstrap') |
        ConvertFrom-Json
    $childTab = @(
        $childBootstrap.tabs |
            Where-Object {
                $_.id -eq $childBootstrap.active_tab_id -and
                $_.parent_id -eq $tabId
            }
    )[0]
    if ($null -eq $childTab -or $childBootstrap.tabs.Count -ne 2) {
        throw (
            'Tabs Add did not create and select one direct child: ' +
            ($childBootstrap | ConvertTo-Json -Depth 8 -Compress)
        )
    }
    $childTitleEditor = [AgenTermRemoteUiNativeTest]::GetDlgItem(
        $gui.MainWindowHandle, 2105
    )
    $childEditCancel = [AgenTermRemoteUiNativeTest]::GetDlgItem(
        $gui.MainWindowHandle, 2108
    )
    if ($childTitleEditor -eq [IntPtr]::Zero -or
        $childEditCancel -eq [IntPtr]::Zero -or
        -not [AgenTermRemoteUiNativeTest]::IsWindowVisible(
            $childTitleEditor
        )) {
        throw 'Tabs Add did not enter the new child inline editor'
    }
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $childEditCancel, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null

    Write-Host 'STEP collapse and expand the server-owned tab tree'
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0201, [IntPtr]::Zero,
        [AgenTermRemoteUiNativeTest]::MousePoint(12, 20)
    ) | Out-Null
    $collapsedBootstrap = Invoke-AgenTerm @('ui-bootstrap') |
        ConvertFrom-Json
    $collapsedRoot = @(
        $collapsedBootstrap.tabs | Where-Object id -eq $tabId
    )[0]
    if (-not $collapsedRoot.collapsed -or
        $collapsedBootstrap.active_tab_id -ne $childId -or
        $collapsedBootstrap.tabs.Count -ne 2) {
        throw (
            'tree collapse did not preserve server tabs and active identity: ' +
            ($collapsedBootstrap | ConvertTo-Json -Depth 8 -Compress)
        )
    }
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0201, [IntPtr]::Zero,
        [AgenTermRemoteUiNativeTest]::MousePoint(12, 20)
    ) | Out-Null
    $expandedBootstrap = Invoke-AgenTerm @('ui-bootstrap') |
        ConvertFrom-Json
    $expandedRoot = @(
        $expandedBootstrap.tabs | Where-Object id -eq $tabId
    )[0]
    if ($expandedRoot.collapsed -or
        $expandedBootstrap.active_tab_id -ne $childId) {
        throw (
            'tree expand changed active identity or remained collapsed: ' +
            ($expandedBootstrap | ConvertTo-Json -Depth 8 -Compress)
        )
    }

    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0201, [IntPtr]::Zero,
        [AgenTermRemoteUiNativeTest]::MousePoint(330, 64)
    ) | Out-Null
    $tabCloseConfirm = [AgenTermRemoteUiNativeTest]::GetDlgItem(
        $gui.MainWindowHandle, 2119
    )
    $tabCloseCancel = [AgenTermRemoteUiNativeTest]::GetDlgItem(
        $gui.MainWindowHandle, 2120
    )
    if ($tabCloseConfirm -eq [IntPtr]::Zero -or
        $tabCloseCancel -eq [IntPtr]::Zero -or
        -not [AgenTermRemoteUiNativeTest]::IsWindowVisible(
            $tabCloseConfirm
        ) -or
        -not [AgenTermRemoteUiNativeTest]::IsWindowVisible(
            $tabCloseCancel
        )) {
        throw 'closing a live remote tab did not open the client-owned confirmation'
    }
    if (-not [string]::IsNullOrWhiteSpace($ScreenshotPath)) {
        $requestedScreenshot = [IO.Path]::GetFullPath($ScreenshotPath)
        $tabCloseScreenshot = Join-Path `
            ([IO.Path]::GetDirectoryName($requestedScreenshot)) `
            "$([IO.Path]::GetFileNameWithoutExtension($requestedScreenshot))-tab-close.png"
        Save-WindowPng -Window $gui.MainWindowHandle `
            -Path $tabCloseScreenshot
    }
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $tabCloseCancel, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    $cancelCloseBootstrap = Invoke-AgenTerm @('ui-bootstrap') |
        ConvertFrom-Json
    if (@(
            $cancelCloseBootstrap.tabs |
                Where-Object id -eq $childId
        ).Count -ne 1 -or
        [AgenTermRemoteUiNativeTest]::IsWindowVisible(
            $tabCloseConfirm
        )) {
        throw 'Tabs close Cancel mutated the tree or left modal controls visible'
    }

    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0201, [IntPtr]::Zero,
        [AgenTermRemoteUiNativeTest]::MousePoint(330, 64)
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $tabCloseConfirm, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    $confirmedCloseBootstrap = Invoke-AgenTerm @('ui-bootstrap') |
        ConvertFrom-Json
    if ($confirmedCloseBootstrap.tabs.Count -ne 1 -or
        $confirmedCloseBootstrap.tabs[0].id -ne $tabId -or
        $confirmedCloseBootstrap.active_tab_id -ne $tabId -or
        [AgenTermRemoteUiNativeTest]::IsWindowVisible(
            $tabCloseConfirm
        )) {
        throw (
            'Tabs close confirmation did not remove only the selected child: ' +
            ($confirmedCloseBootstrap | ConvertTo-Json -Depth 8 -Compress)
        )
    }

    Write-Host 'STEP edit title and note in place with Save and Cancel'
    $editedTitle = "remote-edited-$($run.RunId)"
    $editedNote = "note-$($run.RunId)"
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0201, [IntPtr]::Zero,
        [AgenTermRemoteUiNativeTest]::MousePoint(260, 20)
    ) | Out-Null
    $titleEditor = [AgenTermRemoteUiNativeTest]::GetDlgItem(
        $gui.MainWindowHandle, 2105
    )
    $noteEditor = [AgenTermRemoteUiNativeTest]::GetDlgItem(
        $gui.MainWindowHandle, 2106
    )
    $saveButton = [AgenTermRemoteUiNativeTest]::GetDlgItem(
        $gui.MainWindowHandle, 2107
    )
    $cancelButton = [AgenTermRemoteUiNativeTest]::GetDlgItem(
        $gui.MainWindowHandle, 2108
    )
    if ($titleEditor -eq [IntPtr]::Zero -or
        $noteEditor -eq [IntPtr]::Zero -or
        $saveButton -eq [IntPtr]::Zero -or
        $cancelButton -eq [IntPtr]::Zero -or
        -not [AgenTermRemoteUiNativeTest]::IsWindowVisible($titleEditor) -or
        -not [AgenTermRemoteUiNativeTest]::IsWindowVisible($saveButton)) {
        throw 'replaceable UI did not replace Edit with inline Save/Cancel controls'
    }
    [AgenTermRemoteUiNativeTest]::SendMessageText(
        $titleEditor, 0x000C, [IntPtr]::Zero, $editedTitle
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessageText(
        $noteEditor, 0x000C, [IntPtr]::Zero, $editedNote
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $saveButton, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    $edited = Invoke-AgenTerm @('inspect', '-t', $tabId) |
        ConvertFrom-Json
    if ($edited.windows[0].name -ne $editedTitle -or
        $edited.windows[0].note -ne $editedNote -or
        [AgenTermRemoteUiNativeTest]::IsWindowVisible($titleEditor)) {
        Save-WindowPng -Window $gui.MainWindowHandle `
            -Path (Join-Path $run.RunDirectory 'inline-edit-failure.png')
        throw (
            'inline Save did not persist title/note and return to Edit state: ' +
            "name=$($edited.windows[0].name) note=$($edited.windows[0].note) " +
            "editor_visible=$([AgenTermRemoteUiNativeTest]::IsWindowVisible($titleEditor))"
        )
    }

    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0201, [IntPtr]::Zero,
        [AgenTermRemoteUiNativeTest]::MousePoint(260, 20)
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessageText(
        $titleEditor, 0x000C, [IntPtr]::Zero, ('x' * 4097)
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $saveButton, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    $rejected = Invoke-AgenTerm @('inspect', '-t', $tabId) |
        ConvertFrom-Json
    if ($rejected.windows[0].name -ne $editedTitle -or
        -not [AgenTermRemoteUiNativeTest]::IsWindowVisible($titleEditor)) {
        throw 'inline editor did not reject an oversized title without mutation'
    }
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $cancelButton, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null

    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0201, [IntPtr]::Zero,
        [AgenTermRemoteUiNativeTest]::MousePoint(260, 20)
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessageText(
        $titleEditor, 0x000C, [IntPtr]::Zero, 'must-not-save'
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $cancelButton, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    $cancelled = Invoke-AgenTerm @('inspect', '-t', $tabId) |
        ConvertFrom-Json
    if ($cancelled.windows[0].name -ne $editedTitle -or
        [AgenTermRemoteUiNativeTest]::IsWindowVisible($titleEditor)) {
        throw 'inline Cancel changed the server title or left editor controls visible'
    }

    Write-Host 'STEP scroll a dense Tabs army through its visible scrollbar'
    $sidebarFixtureIds = @()
    foreach ($index in 1..8) {
        $sidebarFixtureIds += (Invoke-AgenTerm @(
                'new-window', '-d', '-P', '-F', '#{window_id}',
                '-n', "sidebar-$index", '--', 'cmd.exe', '/c', 'exit'
            )).Trim()
    }
    if (-not [AgenTermRemoteUiNativeTest]::SetWindowPos(
            $gui.MainWindowHandle, [IntPtr]::Zero, 0, 0, 900, 320,
            0x0016
        )) {
        throw 'SetWindowPos failed for dense Tabs scrollbar fixture'
    }
    $sidebarDeadline = [DateTime]::UtcNow.AddSeconds(3)
    do {
        $sidebarUi = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
        if ([int]$sidebarUi.layout.sidebar.scrollbar.max_offset -gt 0) {
            break
        }
    } while ([DateTime]::UtcNow -lt $sidebarDeadline)
    $sidebarScrollbar = $sidebarUi.layout.sidebar.scrollbar
    if ($null -eq $sidebarScrollbar -or
        [int]$sidebarScrollbar.max_offset -le 0 -or
        [int]$sidebarScrollbar.track.width -ne 12) {
        throw 'dense Tabs did not expose its bounded vertical scrollbar'
    }
    $sidebarX = [int](
        $sidebarScrollbar.thumb.left + ($sidebarScrollbar.thumb.width / 2)
    )
    $sidebarThumbY = [int](
        $sidebarScrollbar.thumb.top + ($sidebarScrollbar.thumb.height / 2)
    )
    $sidebarBottomY = [int](
        $sidebarScrollbar.track.top + $sidebarScrollbar.track.height - 2
    )
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0201, [IntPtr]::Zero,
        [AgenTermRemoteUiNativeTest]::MousePoint($sidebarX, $sidebarThumbY)
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0200, [IntPtr]::Zero,
        [AgenTermRemoteUiNativeTest]::MousePoint($sidebarX, $sidebarBottomY)
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0202, [IntPtr]::Zero,
        [AgenTermRemoteUiNativeTest]::MousePoint($sidebarX, $sidebarBottomY)
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0113, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    $sidebarScrollDeadline = [DateTime]::UtcNow.AddSeconds(2)
    do {
        $sidebarScrolled = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
        if ([int]$sidebarScrolled.layout.sidebar.scrollbar.offset -gt 0) {
            break
        }
    } while ([DateTime]::UtcNow -lt $sidebarScrollDeadline)
    if ([int]$sidebarScrolled.layout.sidebar.scrollbar.offset -le 0) {
        throw 'dragging the Tabs scrollbar did not change its visible row offset'
    }
    [AgenTermRemoteUiNativeTest]::SetWindowPos(
        $gui.MainWindowHandle, [IntPtr]::Zero, 0, 0, 1100, 760, 0x0016
    ) | Out-Null
    foreach ($fixtureId in $sidebarFixtureIds) {
        Invoke-AgenTerm @('kill-window', '-t', $fixtureId) | Out-Null
    }

    $scrollMarker = "AGENTERM_REMOTE_SCROLL_$($run.RunId)_"
    Invoke-AgenTerm @(
        'send-keys', '-t', $tabId, '-l',
        "for /L %i in (1,1,80) do @echo $scrollMarker%i"
    ) | Out-Null
    Invoke-AgenTerm @('send-keys', '-t', $tabId, 'Enter') | Out-Null
    Invoke-AgenTerm @(
        'wait-pane', '-t', $tabId,
        '--contains', "${scrollMarker}80", '--timeout-ms', '5000'
    ) | Out-Null
    Invoke-AgenTerm @(
        'wait-pane', '-t', $tabId,
        '--submit-complete', '--timeout-ms', '5000'
    ) | Out-Null
    $scrollBootstrap = Invoke-AgenTerm @('ui-bootstrap') |
        ConvertFrom-Json
    $scrollTab = @(
        $scrollBootstrap.tabs | Where-Object id -eq $tabId
    )[0]
    if ([int]$scrollTab.screen.max_scrollback -le 0) {
        throw 'screen DTO did not publish the terminal history bound'
    }
    # Drive one public window tick so the client consumes the same completed
    # server screen state before physical scrollbar hit-testing.
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0113, [IntPtr]::Zero, [IntPtr]::Zero
    ) | Out-Null
    $scrollClient = [AgenTermRemoteUiNativeTest+Rect]::new()
    if (-not [AgenTermRemoteUiNativeTest]::GetClientRect(
            $gui.MainWindowHandle, [ref]$scrollClient
        )) {
        throw 'GetClientRect failed for terminal scrollbar'
    }
    $trackHeight = $scrollClient.Bottom - 26 - 104
    $visibleRows = [int]$scrollTab.screen.rows
    $maximumScroll = [int]$scrollTab.screen.max_scrollback
    $thumbHeight = [Math]::Min(
        $trackHeight,
        [Math]::Max(
            24,
            [Math]::Floor(
                $trackHeight * $visibleRows /
                ($visibleRows + $maximumScroll)
            )
        )
    )
    $scrollbarX = $scrollClient.Right - 6
    $bottomThumbY = $trackHeight - [Math]::Floor($thumbHeight / 2)
    $topThumbY = [Math]::Floor($thumbHeight / 2)
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0201, [IntPtr]::Zero,
        [AgenTermRemoteUiNativeTest]::MousePoint(
            $scrollbarX, $bottomThumbY
        )
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0200, [IntPtr]::Zero,
        [AgenTermRemoteUiNativeTest]::MousePoint(
            $scrollbarX, $topThumbY
        )
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0202, [IntPtr]::Zero,
        [AgenTermRemoteUiNativeTest]::MousePoint(
            $scrollbarX, $topThumbY
        )
    ) | Out-Null
    $draggedTop = Invoke-AgenTerm @('inspect', '-t', $tabId) |
        ConvertFrom-Json
    if ([int]$draggedTop.windows[0].scrollback_offset -ne $maximumScroll) {
        throw 'terminal scrollbar drag did not reach the oldest history'
    }
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0201, [IntPtr]::Zero,
        [AgenTermRemoteUiNativeTest]::MousePoint(
            $scrollbarX, $topThumbY
        )
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0200, [IntPtr]::Zero,
        [AgenTermRemoteUiNativeTest]::MousePoint(
            $scrollbarX, $bottomThumbY
        )
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0202, [IntPtr]::Zero,
        [AgenTermRemoteUiNativeTest]::MousePoint(
            $scrollbarX, $bottomThumbY
        )
    ) | Out-Null
    $draggedLive = Invoke-AgenTerm @('inspect', '-t', $tabId) |
        ConvertFrom-Json
    if ([int]$draggedLive.windows[0].scrollback_offset -ne 0) {
        throw 'terminal scrollbar drag did not return to the live viewport'
    }
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x020A,
        [AgenTermRemoteUiNativeTest]::WheelDelta(120),
        [IntPtr]::Zero
    ) | Out-Null
    $scrolled = Invoke-AgenTerm @('inspect', '-t', $tabId) |
        ConvertFrom-Json
    if ($scrolled.windows[0].scrollback_offset -le 0) {
        throw 'replaceable UI mouse wheel did not scroll terminal history'
    }
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x020A,
        [AgenTermRemoteUiNativeTest]::WheelDelta(-120),
        [IntPtr]::Zero
    ) | Out-Null
    $returnedLive = Invoke-AgenTerm @('inspect', '-t', $tabId) |
        ConvertFrom-Json
    if ($returnedLive.windows[0].scrollback_offset -ne 0) {
        throw 'replaceable UI mouse wheel did not return to the live viewport'
    }

    Write-Host 'STEP select terminal text, Copy, and Paste through the UI'
    $copyMarker = "REMOTE_COPY_$($run.RunId)"
    Invoke-AgenTerm @(
        'send-keys', '-t', $tabId, '-l', "echo $copyMarker"
    ) | Out-Null
    Invoke-AgenTerm @('send-keys', '-t', $tabId, 'Enter') | Out-Null
    Invoke-AgenTerm @(
        'wait-pane', '-t', $tabId,
        '--contains', $copyMarker, '--timeout-ms', '5000'
    ) | Out-Null
    $selectionBootstrap = Invoke-AgenTerm @('ui-bootstrap') |
        ConvertFrom-Json
    $selectionTab = @(
        $selectionBootstrap.tabs | Where-Object id -eq $tabId
    )[0]
    $markerRun = @(
        $selectionTab.screen.runs |
            Where-Object { $_.text.Contains($copyMarker) }
    )[-1]
    if ($null -eq $markerRun) {
        throw 'terminal screen snapshot did not expose the copy marker run'
    }
    $markerColumn = [int]$markerRun.column +
        $markerRun.text.IndexOf($copyMarker)
    $clientRect = [AgenTermRemoteUiNativeTest+Rect]::new()
    if (-not [AgenTermRemoteUiNativeTest]::GetClientRect(
            $gui.MainWindowHandle, [ref]$clientRect
        )) {
        throw 'GetClientRect failed for terminal selection'
    }
    $terminalLeft = 360
    $terminalWidth = $clientRect.Right - $terminalLeft - 12
    $terminalHeight = $clientRect.Bottom - 26 - 104
    $cellWidth = [Math]::Max(
        1, [Math]::Floor($terminalWidth / [int]$selectionTab.screen.columns)
    )
    $cellHeight = [Math]::Max(
        1, [Math]::Floor($terminalHeight / [int]$selectionTab.screen.rows)
    )
    $selectionY = [int](
        ([int]$markerRun.row * $cellHeight) + [Math]::Floor($cellHeight / 2)
    )
    $selectionStartX = [int](
        $terminalLeft + ($markerColumn * $cellWidth) +
        [Math]::Floor($cellWidth / 2)
    )
    $selectionEndX = [int](
        $terminalLeft +
        (($markerColumn + $copyMarker.Length - 1) * $cellWidth) +
        [Math]::Floor($cellWidth / 2)
    )
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0201, [IntPtr]::Zero,
        [AgenTermRemoteUiNativeTest]::MousePoint(
            $selectionStartX, $selectionY
        )
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0200, [IntPtr]::Zero,
        [AgenTermRemoteUiNativeTest]::MousePoint(
            $selectionEndX, $selectionY
        )
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0202, [IntPtr]::Zero,
        [AgenTermRemoteUiNativeTest]::MousePoint(
            $selectionEndX, $selectionY
        )
    ) | Out-Null
    if (-not [string]::IsNullOrWhiteSpace($ScreenshotPath)) {
        $requestedScreenshot = [IO.Path]::GetFullPath($ScreenshotPath)
        $selectionScreenshot = Join-Path `
            ([IO.Path]::GetDirectoryName($requestedScreenshot)) `
            "$([IO.Path]::GetFileNameWithoutExtension($requestedScreenshot))-selection.png"
        Save-WindowPng -Window $gui.MainWindowHandle -Path $selectionScreenshot
    }
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0112, [IntPtr]0x1F00, [IntPtr]::Zero
    ) | Out-Null
    $copied = (Get-Clipboard -Raw).TrimEnd("`r", "`n")
    if ($copied -ne $copyMarker) {
        throw "terminal Copy returned '$copied' instead of '$copyMarker'"
    }

    $pasteMarker = "REMOTE_PASTE_$($run.RunId)"
    Set-Clipboard -Value "echo $pasteMarker"
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0112, [IntPtr]0x1F10, [IntPtr]::Zero
    ) | Out-Null
    [AgenTermRemoteUiNativeTest]::SendMessage(
        $gui.MainWindowHandle, 0x0102, [IntPtr]13, [IntPtr]::Zero
    ) | Out-Null
    Invoke-AgenTerm @(
        'wait-pane', '-t', $tabId,
        '--contains', $pasteMarker, '--timeout-ms', '5000'
    ) | Out-Null

    Write-Host 'STEP reconnect the same GUI process across a server restart'
    $reconnectGuiPid = $gui.Id
    $reconnectGuiHandle = [int64]$gui.MainWindowHandle
    $oldServerPid = [int]$replacementBootstrap.server_pid
    $oldServerEpoch = [string]$replacementBootstrap.server_epoch
    $oldServer = Get-Process -Id $oldServerPid -ErrorAction Stop
    Invoke-AgenTerm @('shutdown') | Out-Null
    if (-not $oldServer.WaitForExit(5000)) {
        throw 'old headless server did not exit after public shutdown'
    }
    $restartStderr = Join-Path $run.RunDirectory `
        'remote-ui-restarted-server-stderr.txt'
    $restartedServer = Start-Process -FilePath $ServerExe `
        -ArgumentList @('--address', $run.Address) `
        -RedirectStandardError $restartStderr `
        -PassThru -WindowStyle Hidden
    Register-SmokeOwnedProcess -Context $run -Id $restartedServer.Id `
        -Kind 'server' -Address $run.Address

    $reconnected = $false
    $reconnectDeadline = [DateTime]::UtcNow.AddSeconds(15)
    do {
        $gui.Refresh()
        $leaseOutput = @(
            & $CliExe --address $run.Address ui-lease status 2>&1
        )
        if ($LASTEXITCODE -eq 0) {
            $reconnectedLease = ($leaseOutput -join "`n") |
                ConvertFrom-Json
            if ($reconnectedLease.attached -and
                $reconnectedLease.client_pid -eq $reconnectGuiPid -and
                $gui.MainWindowHandle -eq $reconnectGuiHandle) {
                $reconnected = $true
                break
            }
        }
        Start-Sleep -Milliseconds 50
    } while ([DateTime]::UtcNow -lt $reconnectDeadline)
    if (-not $reconnected) {
        $restartError = Get-Content -LiteralPath $restartStderr -Raw `
            -ErrorAction SilentlyContinue
        throw (
            'replaceable UI did not reconnect in place after server restart: ' +
            "gui_pid=$reconnectGuiPid hwnd=$reconnectGuiHandle " +
            "new_server_pid=$($restartedServer.Id)`n$restartError"
        )
    }
    $reconnectedBootstrap = Invoke-AgenTerm @('ui-bootstrap') |
        ConvertFrom-Json
    $reconnectedLease = Invoke-AgenTerm @('ui-lease', 'status') |
        ConvertFrom-Json
    if ($gui.Id -ne $reconnectGuiPid -or
        $gui.MainWindowHandle -ne $reconnectGuiHandle -or
        $reconnectedBootstrap.server_pid -ne $restartedServer.Id -or
        $reconnectedBootstrap.server_pid -eq $oldServerPid -or
        $reconnectedBootstrap.server_epoch -eq $oldServerEpoch -or
        $reconnectedLease.client_pid -ne $reconnectGuiPid -or
        $reconnectedLease.position.server_epoch -ne
            $reconnectedBootstrap.server_epoch) {
        throw 'in-place reconnect did not expose the new causal server identity'
    }
    Sync-SmokeOwnedServers -Context $run

    if ([string]::IsNullOrWhiteSpace($ScreenshotPath)) {
        $ScreenshotPath = Join-Path $run.RunDirectory 'remote-ui.png'
    } else {
        $ScreenshotPath = [IO.Path]::GetFullPath($ScreenshotPath)
    }
    Save-WindowPng -Window $gui.MainWindowHandle -Path $ScreenshotPath
    $relayClose = Invoke-AgenTerm @('ui-action', 'close-window') |
        ConvertFrom-Json
    if ($relayClose.modal.kind -ne 'confirm-window-close') {
        throw 'relayed close-window did not open the native confirmation'
    }
    $relayDetached = Invoke-AgenTerm @(
        'ui-action', 'keep-server-running'
    ) | ConvertFrom-Json
    if ($relayDetached.window.visible -ne $false -or
        $relayDetached.window.detached -ne $true -or
        $relayDetached.layout.composer.visible -ne $false) {
        throw 'relayed Keep Server Running did not return detached UI facts'
    }
    if (-not $gui.WaitForExit(5000)) {
        throw 'replacement GUI did not release its lease through Keep Server Running'
    }
    $replacementLeaseAfter = Invoke-AgenTerm @('ui-lease', 'status') |
        ConvertFrom-Json
    if ($replacementLeaseAfter.attached) {
        throw 'replacement GUI left its interactive lease attached'
    }

    Write-Host 'STEP stop the server through the third close choice'
    $stopStderr = Join-Path $run.RunDirectory 'remote-ui-stop-stderr.txt'
    $stopGui = Start-Process -FilePath $GuiExe `
        -ArgumentList @(
            '--ui-client', '--no-activate', '--address', $run.Address
        ) `
        -RedirectStandardError $stopStderr `
        -PassThru -WindowStyle Normal
    Register-SmokeOwnedProcess -Context $run -Id $stopGui.Id `
        -Kind 'gui' -Address $run.Address
    $stopReady = $false
    $stopDeadline = [DateTime]::UtcNow.AddSeconds(10)
    do {
        $stopGui.Refresh()
        $stopLease = Invoke-AgenTerm @('ui-lease', 'status') |
            ConvertFrom-Json
        if ($stopGui.MainWindowHandle -ne 0 -and
            $stopLease.attached -and
            $stopLease.client_pid -eq $stopGui.Id) {
            $stopReady = $true
            break
        }
        Start-Sleep -Milliseconds 50
    } while ([DateTime]::UtcNow -lt $stopDeadline)
    if (-not $stopReady) {
        throw 'final replaceable UI did not acquire the lease for stop-server proof'
    }
    $stopModal = Invoke-AgenTerm @('ui-action', 'close-window') |
        ConvertFrom-Json
    if ($stopModal.modal.kind -ne 'confirm-window-close') {
        throw 'relayed stop proof did not open the window-close confirmation'
    }
    Invoke-AgenTerm @('ui-action', 'stop-server-and-exit') | Out-Null
    if (-not $stopGui.WaitForExit(5000)) {
        throw 'Stop Server & Exit did not close its UI'
    }
    if (-not $restartedServer.WaitForExit(5000)) {
        throw 'Stop Server & Exit left the independent server running'
    }
    Sync-SmokeOwnedServers -Context $run

    Write-Evidence 'ui.replaceable-client'
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
Write-Host (
    'PASS: replaceable GUI attaches, renders, detaches, preserves PTYs, ' +
    'and reconnects in place across server restart'
)
