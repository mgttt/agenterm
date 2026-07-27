param(
    [string]$Exe = (Join-Path $PSScriptRoot '..\dist\agenterm-cli.exe'),
    [switch]$ListEvidence
)

$ErrorActionPreference = 'Stop'
$declaredEvidence = @(
    'ux.hierarchical-tabs'
    'ux.live-close-confirmation'
    'ux.locale-consistency'
    'ux.mouse-scrollback'
    'ux.persistent-workspace'
    'ux.semantic-ui-automation'
    'ux.semantic-window-control'
    'ux.settings-isolation'
    'ux.terminal-selection-copy'
)
if ($ListEvidence) {
    $declaredEvidence
    exit 0
}

function Write-Evidence {
    param([Parameter(Mandatory = $true)][string]$Id)
    if ($declaredEvidence -notcontains $Id) {
        throw "UX smoke emitted undeclared evidence ID: $Id"
    }
    Write-Host "EVIDENCE $Id"
}

$Exe = [IO.Path]::GetFullPath($Exe)
$GuiExe = Join-Path (Split-Path -Parent $Exe) 'agenterm.exe'
if (-not (Test-Path -LiteralPath $Exe)) {
    throw "AgenTerm executable not found: $Exe"
}
if (-not (Test-Path -LiteralPath $GuiExe)) {
    throw "AgenTerm GUI executable not found: $GuiExe"
}

$previousAddress = $env:AGENTERM_IPC_ADDRESS
$previousSettingsPath = $env:AGENTERM_SETTINGS_PATH
$previousWorkspace = $env:AGENTERM_WORKSPACE_PATH
$env:AGENTERM_IPC_ADDRESS = "127.0.0.1:$((47000 + ($PID % 1000)))"
$workspaceFile = Join-Path $env:TEMP "agenterm-ux-$PID.json"
$settingsFile = Join-Path $env:TEMP "agenterm-settings-$PID.json"
$env:AGENTERM_WORKSPACE_PATH = $workspaceFile
$env:AGENTERM_SETTINGS_PATH = $settingsFile
$name = "ux-smoke-$PID"
$token = "AGENTERM_UX_$PID"
$draftFile = Join-Path $env:TEMP "$name.txt"

function Invoke-AgenTerm {
    param([string[]]$CommandArgs)
    $output = & $Exe @CommandArgs 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "agenterm $($CommandArgs -join ' ') failed:`n$($output -join "`n")"
    }
    return ($output -join "`n")
}

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class AgenTermNativeTest {
    [StructLayout(LayoutKind.Sequential)]
    private struct POINT {
        public int X;
        public int Y;
    }

    [DllImport("user32.dll")]
    private static extern bool ClientToScreen(IntPtr window, ref POINT point);

    [DllImport("user32.dll")]
    private static extern IntPtr SendMessageW(
        IntPtr window, uint message, UIntPtr wParam, IntPtr lParam);

    private static IntPtr PointParam(int x, int y) {
        return (IntPtr)(((y & 0xffff) << 16) | (x & 0xffff));
    }

    public static void MouseWheel(IntPtr window, int clientX, int clientY, short delta) {
        POINT point = new POINT { X = clientX, Y = clientY };
        if (!ClientToScreen(window, ref point)) {
            throw new InvalidOperationException("ClientToScreen failed");
        }
        ulong wheel = ((ulong)(ushort)delta) << 16;
        SendMessageW(window, 0x020A, (UIntPtr)wheel, PointParam(point.X, point.Y));
    }

    public static void Drag(
        IntPtr window, int startX, int startY, int endX, int endY) {
        SendMessageW(window, 0x0201, UIntPtr.Zero, PointParam(startX, startY));
        SendMessageW(window, 0x0200, (UIntPtr)1, PointParam(endX, endY));
        SendMessageW(window, 0x0202, UIntPtr.Zero, PointParam(endX, endY));
    }
}
'@

try {
    Write-Host 'STEP create and discover stable tab'
    Invoke-AgenTerm @('new-window', '-d', '-n', $name) | Out-Null
    $snapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $tab = $snapshot.tabs | Where-Object name -eq $name
    if ($null -eq $tab -or $tab.state -ne 'running') {
        throw 'ui-snapshot did not expose the running test tab'
    }
    if ($snapshot.window.title -notmatch '^AgenTerm-\d+\.\d+\.\d+:\d+$') {
        throw "Window title is not versioned: $($snapshot.window.title)"
    }
    if ($snapshot.layout.status_bar.height -le 0 -or
        $snapshot.layout.status_bar.provider -ne 'placeholder') {
        throw 'Bottom status bar was not exposed through ui-snapshot'
    }
    if ($snapshot.locale.id -ne 'en-US' -or
        $snapshot.locale.controls.send -ne 'Send' -or
        @($snapshot.locale.controls.PSObject.Properties.Value) -contains '发送') {
        throw 'built-in control labels did not use the declared English locale'
    }
    Write-Evidence 'ux.locale-consistency'
    $id = $tab.id

    Write-Host 'STEP semantic window state and client resize'
    $originalWidth = [int]$snapshot.window.client_width
    $originalHeight = [int]$snapshot.window.client_height
    $originalRows = [int]$snapshot.layout.terminal.rows
    $originalCols = [int]$snapshot.layout.terminal.cols
    Invoke-AgenTerm @('ui-action', 'window-minimize') | Out-Null
    $minimized = Invoke-AgenTerm @(
        'wait-ui', '--window-state', 'minimized', '--timeout-ms', '5000'
    ) | ConvertFrom-Json
    if ($minimized.layout.terminal.rows -ne $originalRows -or
        $minimized.layout.terminal.cols -ne $originalCols) {
        throw 'minimizing changed the last committed PTY grid'
    }
    Invoke-AgenTerm @('ui-action', 'window-restore') | Out-Null
    Invoke-AgenTerm @('wait-ui', '--window-state', 'restored') | Out-Null
    Invoke-AgenTerm @('ui-action', 'window-maximize') | Out-Null
    Invoke-AgenTerm @('wait-ui', '--window-state', 'maximized') | Out-Null
    Invoke-AgenTerm @('ui-action', 'window-restore') | Out-Null
    Invoke-AgenTerm @('wait-ui', '--window-state', 'restored') | Out-Null
    $resizedWidth = if ($originalWidth -gt 720) {
        $originalWidth - 80
    } else {
        $originalWidth + 80
    }
    $resizedHeight = if ($originalHeight -gt 540) {
        $originalHeight - 60
    } else {
        $originalHeight + 60
    }
    Invoke-AgenTerm @(
        'ui-action', 'window-resize',
        '--width', "$resizedWidth", '--height', "$resizedHeight"
    ) | Out-Null
    $resized = Invoke-AgenTerm @(
        'wait-ui', '--client-width', "$resizedWidth",
        '--client-height', "$resizedHeight", '--timeout-ms', '5000'
    ) | ConvertFrom-Json
    if ($resized.layout.terminal.rows -eq $originalRows -and
        $resized.layout.terminal.cols -eq $originalCols) {
        throw 'client resize did not update the PTY grid'
    }
    Invoke-AgenTerm @(
        'ui-action', 'window-resize',
        '--width', "$originalWidth", '--height', "$originalHeight"
    ) | Out-Null
    Invoke-AgenTerm @(
        'wait-ui', '--client-width', "$originalWidth",
        '--client-height', "$originalHeight", '--timeout-ms', '5000'
    ) | Out-Null
    Write-Evidence 'ux.semantic-window-control'

    Write-Host 'STEP hierarchical tab team'
    $childName = "worker-$PID"
    $grandchildName = "reviewer-$PID"
    Invoke-AgenTerm @('new-window', '-d', '-n', $childName, '--parent', $id) | Out-Null
    $snapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $child = $snapshot.tabs | Where-Object name -eq $childName
    if ($child.parent_id -ne $id -or $child.depth -ne 1) {
        throw 'Child tab did not appear under the team root'
    }
    Invoke-AgenTerm @(
        'new-window', '-d', '-n', $grandchildName, '--parent', $child.id
    ) | Out-Null
    $snapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $grandchild = $snapshot.tabs | Where-Object name -eq $grandchildName
    if ($grandchild.parent_id -ne $child.id -or $grandchild.depth -ne 2) {
        throw 'Grandchild tab did not expose the expected tree depth'
    }
    $tree = Invoke-AgenTerm @('list-tab-tree')
    if ($tree.IndexOf($childName) -lt $tree.IndexOf($name) -or
        $tree.IndexOf($grandchildName) -lt $tree.IndexOf($childName)) {
        throw "list-tab-tree was not in preorder:`n$tree"
    }
    $snapshot = Invoke-AgenTerm @('ui-action', 'select-tab', '-t', $id) | ConvertFrom-Json
    $tab = $snapshot.tabs | Where-Object id -eq $id
    if ($tab.actions.new_child.width -le 0 -or $tab.actions.edit.width -le 0) {
        throw 'Tree node did not expose direct add-child and edit actions'
    }
    $snapshot = Invoke-AgenTerm @('ui-action', 'toggle-tree', '-t', $id) | ConvertFrom-Json
    $child = $snapshot.tabs | Where-Object name -eq $childName
    if (-not $tab.has_children -or -not ($snapshot.tabs | Where-Object id -eq $id).collapsed -or
        $child.visible) {
        throw 'Collapsing a tree node did not hide its descendants'
    }
    Invoke-AgenTerm @('ui-action', 'toggle-tree', '-t', $id) | Out-Null

    Write-Host 'STEP direct node add-child and editor'
    $snapshot = Invoke-AgenTerm @('ui-action', 'new-child', '-t', $id) | ConvertFrom-Json
    $directChild = $snapshot.tabs | Where-Object {
        $_.parent_id -eq $id -and $_.name -eq 'New child'
    }
    if ($null -eq $directChild -or $snapshot.focus.surface -ne 'note-editor') {
        throw 'Direct add-child did not create a child and open its editor'
    }
    $directName = "direct-worker-$PID"
    Invoke-AgenTerm @(
        'set-composer', '-t', $directChild.id, "$directName`ncreated from node editor"
    ) | Out-Null
    $snapshot = Invoke-AgenTerm @('ui-action', 'composer-send') | ConvertFrom-Json
    $directChild = $snapshot.tabs | Where-Object id -eq $directChild.id
    if ($directChild.name -ne $directName -or
        $directChild.note -ne 'created from node editor') {
        throw 'Direct node editor did not save the name and note'
    }
    Invoke-AgenTerm @('kill-window', '-t', $directChild.id) | Out-Null

    $savedErrorPreference = $ErrorActionPreference
    $ErrorActionPreference = 'SilentlyContinue'
    & $Exe set-tab-parent -t $id --parent $grandchild.id 2>$null | Out-Null
    $cycleExitCode = $LASTEXITCODE
    $ErrorActionPreference = $savedErrorPreference
    if ($cycleExitCode -eq 0) {
        throw 'set-tab-parent accepted a parent cycle'
    }

    Invoke-AgenTerm @('kill-window', '-t', $child.id) | Out-Null
    $snapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $grandchild = $snapshot.tabs | Where-Object name -eq $grandchildName
    if ($grandchild.parent_id -ne $id -or $grandchild.depth -ne 1) {
        throw 'Closing a parent did not safely promote its child'
    }
    Invoke-AgenTerm @('kill-window', '-t', $grandchild.id) | Out-Null
    Write-Evidence 'ux.hierarchical-tabs'

    Write-Host 'STEP two-line tab metadata'
    $note = "build verification $PID"
    Invoke-AgenTerm @('set-tab-note', '-t', $id, $note) | Out-Null
    $roundTripNote = Invoke-AgenTerm @('show-tab-note', '-t', $id)
    if ($roundTripNote -ne $note) {
        throw "Tab note round trip mismatch: [$roundTripNote]"
    }
    $snapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $tab = $snapshot.tabs | Where-Object id -eq $id
    if ($tab.note -ne $note -or [string]::IsNullOrWhiteSpace($tab.terminal_title)) {
        throw 'ui-snapshot did not expose tab note and terminal title'
    }

    Write-Host 'STEP lossless file composer and draft state'
    [IO.File]::WriteAllText($draftFile, "echo $token")
    Invoke-AgenTerm @('set-composer', '-t', $id, '--file', $draftFile) | Out-Null
    $snapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $tab = $snapshot.tabs | Where-Object id -eq $id
    if (-not $tab.draft) {
        throw 'ui-snapshot did not expose the composer draft'
    }

    Write-Host 'STEP semantic focus and composer send'
    Invoke-AgenTerm @('ui-action', 'select-tab', '-t', $id) | Out-Null
    Invoke-AgenTerm @('focus', 'composer', '-t', $id) | Out-Null
    Invoke-AgenTerm @('wait-ui', '--active', $id, '--focus', 'composer') | Out-Null
    Invoke-AgenTerm @('ui-action', 'composer-send', '-t', $id) | Out-Null
    Invoke-AgenTerm @(
        'wait-pane', '-t', $id, '--contains', $token,
        '--submit-complete', '--timeout-ms', '10000'
    ) | Out-Null
    Write-Evidence 'ux.semantic-ui-automation'

    Write-Host 'STEP composer edit shortcut routing'
    # The key-to-edit-command mapping is unit tested; this public focus assertion
    # guards the native EDIT surface to which Ctrl+A/C/X/V are dispatched.
    $snapshot = Invoke-AgenTerm @('focus', 'composer', '-t', $id) | ConvertFrom-Json
    if ($snapshot.focus.surface -ne 'composer') {
        throw 'composer shortcut target was not the focused native edit control'
    }

    Write-Host 'STEP physical mouse wheel and draggable terminal scrollbar'
    $scrollPrefix = "AGENTERM_UX_SCROLL_$PID"
    Invoke-AgenTerm @(
        'send-keys', '-l', '-t', $id,
        "for /L %i in (1,1,80) do @echo $scrollPrefix`_%i"
    ) | Out-Null
    Invoke-AgenTerm @('send-keys', '-t', $id, 'Enter') | Out-Null
    Invoke-AgenTerm @(
        'wait-pane', '-t', $id, '--contains', "$scrollPrefix`_80",
        '--timeout-ms', '10000'
    ) | Out-Null
    $snapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $scrollbar = $snapshot.layout.terminal.scrollbar
    $tab = $snapshot.tabs | Where-Object id -eq $id
    if ($null -eq $scrollbar -or $scrollbar.max_offset -le 0 -or
        $scrollbar.track.width -ne 12 -or $tab.scrollback_offset -ne 0) {
        throw 'ui-snapshot did not expose a usable terminal scrollbar'
    }
    $window = @(
        Get-Process agenterm -ErrorAction SilentlyContinue |
            Where-Object MainWindowTitle -eq $snapshot.window.title |
            Select-Object -First 1 -ExpandProperty MainWindowHandle
    )
    if ($window.Count -gt 0) {
        $window = [IntPtr]$window[0]
    } else {
        $window = [IntPtr]::Zero
    }
    if ($window -eq [IntPtr]::Zero) {
        throw 'could not resolve the public AgenTerm window for mouse regression'
    }
    [AgenTermNativeTest]::MouseWheel(
        $window,
        [int]$snapshot.layout.terminal.x + 10,
        [int]$snapshot.layout.terminal.y + 10,
        120
    )
    $snapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $tab = $snapshot.tabs | Where-Object id -eq $id
    if ($tab.scrollback_offset -ne 3) {
        throw "one mouse-wheel notch scrolled $($tab.scrollback_offset) rows instead of 3"
    }
    $scrollbar = $snapshot.layout.terminal.scrollbar
    [AgenTermNativeTest]::Drag(
        $window,
        [int]($scrollbar.thumb.x + ($scrollbar.thumb.width / 2)),
        [int]($scrollbar.thumb.y + ($scrollbar.thumb.height / 2)),
        [int]($scrollbar.thumb.x + ($scrollbar.thumb.width / 2)),
        [int]($scrollbar.track.y + $scrollbar.track.height - 1)
    )
    $snapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $tab = $snapshot.tabs | Where-Object id -eq $id
    if ($tab.scrollback_offset -ne 0) {
        throw 'dragging the terminal scrollbar to the bottom did not restore live output'
    }
    Write-Evidence 'ux.mouse-scrollback'

    Write-Host 'STEP terminal text selection and system clipboard copy'
    $selectionToken = "$scrollPrefix`_80"
    $capture = Invoke-AgenTerm @('capture-pane', '-p', '-t', $id)
    $captureLines = @($capture -split "`n" | ForEach-Object { $_.TrimEnd("`r") })
    $selectionRow = -1
    $selectionColumn = -1
    for ($lineIndex = 0; $lineIndex -lt $captureLines.Count; $lineIndex++) {
        $column = $captureLines[$lineIndex].IndexOf($selectionToken)
        if ($column -ge 0) {
            $selectionRow = $lineIndex
            $selectionColumn = $column
            break
        }
    }
    if ($selectionRow -lt 0) {
        throw 'could not locate the selection token in the visible terminal viewport'
    }
    $snapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $terminal = $snapshot.layout.terminal
    $cellWidth = [int]([Math]::Floor($terminal.viewport_width / $terminal.cols))
    $cellHeight = [int]([Math]::Floor($terminal.height / $terminal.rows))
    if ($cellWidth -le 0 -or $cellHeight -le 0) {
        throw 'terminal snapshot did not expose usable cell geometry'
    }
    $startX = [int]$terminal.x + ($selectionColumn * $cellWidth) + 1
    $endColumn = $selectionColumn + $selectionToken.Length - 1
    $endX = [int]$terminal.x + ($endColumn * $cellWidth) + [Math]::Max(1, $cellWidth - 2)
    $selectionY = [int]$terminal.y + ($selectionRow * $cellHeight) + [Math]::Max(1, [int]($cellHeight / 2))
    [AgenTermNativeTest]::Drag($window, $startX, $selectionY, $endX, $selectionY)
    $snapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $tab = $snapshot.tabs | Where-Object id -eq $id
    if ($null -eq $tab.selection -or $tab.selection.dragging -or
        $tab.selection.start.row -ne $selectionRow -or
        $tab.selection.start.col -ne $selectionColumn -or
        $tab.selection.end.col -ne $endColumn) {
        throw (
            "mouse drag selection mismatch: expected " +
            "$selectionRow,$selectionColumn..$selectionRow,$endColumn; actual " +
            "$($tab.selection | ConvertTo-Json -Compress)"
        )
    }
    Set-Clipboard -Value 'AGENTERM_CLIPBOARD_SENTINEL'
    Invoke-AgenTerm @('ui-action', 'copy-selection') | Out-Null
    $copied = Get-Clipboard -Raw
    if ($copied -ne $selectionToken) {
        throw "terminal selection copied unexpected text: '$copied'"
    }
    Write-Evidence 'ux.terminal-selection-copy'

    Write-Host 'STEP live close requires confirmation and cancel is safe'
    $snapshot = Invoke-AgenTerm @('ui-action', 'close-tab', '-t', $id) | ConvertFrom-Json
    if ($snapshot.modal.kind -ne 'confirm-close-live' -or $snapshot.modal.window_id -ne $id) {
        throw 'live close did not expose the confirmation modal'
    }
    Invoke-AgenTerm @('ui-action', 'cancel') | Out-Null
    $snapshot = Invoke-AgenTerm @('wait-ui', '-t', $id, '--tab-state', 'running') | ConvertFrom-Json
    if ($null -ne $snapshot.modal) {
        throw 'cancel did not clear the confirmation modal'
    }
    Write-Evidence 'ux.live-close-confirmation'

    Write-Host 'STEP settings discovery and modal'
    $settings = Invoke-AgenTerm @('get-settings') | ConvertFrom-Json
    if ($settings.terminal_font_size -lt 8 -or
        $settings.recommended_cjk_font -ne 'Sarasa Fixed SC' -or
        [IO.Path]::GetFullPath($settings.config_path) -ne
            [IO.Path]::GetFullPath($settingsFile)) {
        throw 'get-settings did not expose isolated font settings and CJK recommendation'
    }
    Write-Evidence 'ux.settings-isolation'
    $snapshot = Invoke-AgenTerm @('ui-action', 'open-settings') | ConvertFrom-Json
    if ($snapshot.modal.kind -ne 'settings' -or
        $snapshot.focus.surface -ne 'settings') {
        throw 'settings modal/focus was not exposed'
    }
    Invoke-AgenTerm @('ui-action', 'cancel') | Out-Null

    Write-Host 'STEP dead tab remains and closes without confirmation'
    Invoke-AgenTerm @('send-keys', '-t', $id, 'exit', 'Enter') | Out-Null
    Invoke-AgenTerm @('wait-ui', '-t', $id, '--tab-state', 'dead', '--timeout-ms', '10000') | Out-Null
    $snapshot = Invoke-AgenTerm @('ui-action', 'close-tab', '-t', $id) | ConvertFrom-Json
    if ($snapshot.tabs.id -contains $id -or $null -ne $snapshot.modal) {
        throw 'dead tab was not closed immediately'
    }

    Write-Host 'STEP protocol discovery'
    $protocol = Invoke-AgenTerm @('protocol-info') | ConvertFrom-Json
    if ($protocol.protocol_version -ne 1 -or -not $protocol.features.semantic_ui_automation) {
        throw 'protocol-info did not advertise semantic UI automation'
    }

    Write-Host 'STEP workspace survives a normal application restart'
    $persistName = "persistent-$PID"
    Invoke-AgenTerm @('new-window', '-d', '-n', $persistName) | Out-Null
    Invoke-AgenTerm @('set-tab-note', '-t', $persistName, 'restored note') | Out-Null
    Invoke-AgenTerm @('set-composer', '-t', $persistName, 'restored draft') | Out-Null
    Invoke-AgenTerm @('select-window', '-t', $persistName) | Out-Null
    Invoke-AgenTerm @('shutdown') | Out-Null
    for ($attempt = 0; $attempt -lt 50; $attempt++) {
        & $Exe ui-snapshot 2>$null | Out-Null
        if ($LASTEXITCODE -ne 0) { break }
        Start-Sleep -Milliseconds 50
    }
    Start-Process -FilePath $GuiExe | Out-Null
    $ready = $false
    for ($attempt = 0; $attempt -lt 100; $attempt++) {
        & $Exe ui-snapshot 2>$null | Out-Null
        if ($LASTEXITCODE -eq 0) {
            $ready = $true
            break
        }
        Start-Sleep -Milliseconds 50
    }
    if (-not $ready) {
        throw 'Restored AgenTerm server did not become ready'
    }
    $snapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $restored = $snapshot.tabs | Where-Object name -eq $persistName
    if ($null -eq $restored -or $restored.note -ne 'restored note' -or
        -not $restored.draft -or -not $restored.active) {
        throw 'Workspace tab metadata and active selection did not survive restart'
    }
    Write-Evidence 'ux.persistent-workspace'
    Write-Host 'PASS: UX state, two-line tabs, settings, composer, focus, safe close, dead close, protocol discovery'
}
finally {
    Remove-Item -LiteralPath $draftFile -ErrorAction SilentlyContinue
    & $Exe kill-server 2>$null | Out-Null
    Remove-Item -LiteralPath $workspaceFile -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $settingsFile -ErrorAction SilentlyContinue
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
    if ($null -eq $previousSettingsPath) {
        Remove-Item Env:AGENTERM_SETTINGS_PATH -ErrorAction SilentlyContinue
    }
    else {
        $env:AGENTERM_SETTINGS_PATH = $previousSettingsPath
    }
}
