param(
    [string]$Exe = (Join-Path $PSScriptRoot '..\dist\agenterm-cli.exe'),
    [switch]$ListEvidence
)

$ErrorActionPreference = 'Stop'
$declaredEvidence = @(
    'ux.adaptive-tabs'
    'ux.hierarchical-tabs'
    'ux.detach-first-window-close'
    'ux.live-close-confirmation'
    'ux.locale-consistency'
    'ux.keyboard-surface-navigation'
    'ux.modal-wait'
    'ux.mouse-scrollback'
    'ux.no-activate-launch'
    'ux.persistent-workspace'
    'ux.semantic-ui-automation'
    'ux.semantic-window-control'
    'ux.settings-isolation'
    'ux.system-menu-clipboard'
    'ux.terminal-selection-copy'
    'ux.working-context-cwd'
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
$stopAddress = "127.0.0.1:$((48000 + ($PID % 1000)))"
$stopWorkspaceFile = Join-Path $env:TEMP "agenterm-stop-ux-$PID.json"
$stopSettingsFile = Join-Path $env:TEMP "agenterm-stop-settings-$PID.json"
$noActivateAddress = "127.0.0.1:$((49000 + ($PID % 1000)))"
$noActivateWorkspaceFile = Join-Path $env:TEMP "agenterm-no-activate-ux-$PID.json"
$noActivateSettingsFile = Join-Path $env:TEMP "agenterm-no-activate-settings-$PID.json"
$env:AGENTERM_WORKSPACE_PATH = $workspaceFile
$env:AGENTERM_SETTINGS_PATH = $settingsFile
$name = "ux-smoke-$PID"
$token = "AGENTERM_UX_$PID"
$draftFile = Join-Path $env:TEMP "$name.txt"
$foregroundHost = [IntPtr]::Zero

function Invoke-AgenTerm {
    param([string[]]$CommandArgs)
    $previousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $output = & $Exe @CommandArgs 2>&1
        $exitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previousPreference
    }
    if ($exitCode -ne 0) {
        throw "agenterm $($CommandArgs -join ' ') failed:`n$($output -join "`n")"
    }
    return ($output -join "`n")
}

function Invoke-AgenTermAt {
    param(
        [Parameter(Mandatory = $true)][string]$Address,
        [string[]]$CommandArgs
    )
    $previousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $output = & $Exe '--address' $Address @CommandArgs 2>&1
        $exitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previousPreference
    }
    if ($exitCode -ne 0) {
        throw "agenterm --address $Address $($CommandArgs -join ' ') failed:`n$($output -join "`n")"
    }
    return ($output -join "`n")
}

function Test-AgenTermReady {
    $previousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $null = & $Exe ui-snapshot 2>&1
        return $LASTEXITCODE -eq 0
    }
    finally {
        $ErrorActionPreference = $previousPreference
    }
}

function Get-IsolatedServer {
    param([string]$Address = $env:AGENTERM_IPC_ADDRESS)
    $instances = Invoke-AgenTerm @('list-instances', '--json') | ConvertFrom-Json
    $matches = @($instances | Where-Object {
        $_.address -eq $Address -and $_.status -eq 'running'
    })
    if ($matches.Count -ne 1) {
        throw "expected one running isolated server, found $($matches.Count)"
    }
    return $matches[0]
}

function Get-PaneSnapshot {
    param(
        [Parameter(Mandatory = $true)][string]$Target,
        [string]$Address
    )
    $inspection = if ([string]::IsNullOrWhiteSpace($Address)) {
        Invoke-AgenTerm @('pane-snapshot', '-t', $Target) | ConvertFrom-Json
    } else {
        Invoke-AgenTermAt -Address $Address `
            -CommandArgs @('pane-snapshot', '-t', $Target) | ConvertFrom-Json
    }
    $panes = @($inspection.windows)
    if ($panes.Count -ne 1 -or $panes[0].id -ne $Target) {
        throw "pane-snapshot did not return exactly $Target"
    }
    return $panes[0]
}

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;

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

    [DllImport("user32.dll", EntryPoint = "SendMessageW")]
    private static extern IntPtr SendMessagePointer(
        IntPtr window, uint message, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    private static extern bool SetForegroundWindow(IntPtr window);

    [DllImport("user32.dll")]
    private static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr CreateWindowExW(
        uint exStyle, string className, string title, uint style,
        int x, int y, int width, int height, IntPtr parent, IntPtr menu,
        IntPtr instance, IntPtr parameter);

    [DllImport("user32.dll")]
    private static extern bool DestroyWindow(IntPtr window);

    [DllImport("user32.dll")]
    private static extern bool ShowWindow(IntPtr window, int command);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr FindWindowExW(
        IntPtr parent, IntPtr after, string className, string windowName);

    [DllImport("user32.dll")]
    private static extern void keybd_event(
        byte virtualKey, byte scanCode, uint flags, UIntPtr extraInfo);

    [DllImport("user32.dll")]
    private static extern uint GetWindowThreadProcessId(
        IntPtr window, out uint processId);

    [DllImport("user32.dll")]
    private static extern bool GetGUIThreadInfo(
        uint threadId, ref GUITHREADINFO info);

    [StructLayout(LayoutKind.Sequential)]
    private struct GUITHREADINFO {
        public uint cbSize;
        public uint flags;
        public IntPtr hwndActive;
        public IntPtr hwndFocus;
        public IntPtr hwndCapture;
        public IntPtr hwndMenuOwner;
        public IntPtr hwndMoveSize;
        public IntPtr hwndCaret;
        public RECT rcCaret;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct RECT {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [DllImport("user32.dll")]
    private static extern IntPtr GetSystemMenu(IntPtr window, bool revert);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern int GetMenuStringW(
        IntPtr menu, uint item, StringBuilder text, int length, uint flags);

    [DllImport("user32.dll")]
    private static extern uint GetMenuState(IntPtr menu, uint item, uint flags);

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

    public static void Click(IntPtr window, int x, int y) {
        SendMessageW(window, 0x0201, UIntPtr.Zero, PointParam(x, y));
        SendMessageW(window, 0x0202, UIntPtr.Zero, PointParam(x, y));
    }

    public static void ClickButton(IntPtr window, string label) {
        IntPtr button = FindWindowExW(window, IntPtr.Zero, "BUTTON", label);
        if (button == IntPtr.Zero) {
            throw new InvalidOperationException("button was not found: " + label);
        }
        SendMessageW(button, 0x00F5, UIntPtr.Zero, IntPtr.Zero);
    }

    public static void DoubleClick(IntPtr window, int x, int y) {
        SendMessageW(window, 0x0203, (UIntPtr)1, PointParam(x, y));
    }

    public static string SystemMenuLabel(IntPtr window, uint command) {
        IntPtr menu = GetSystemMenu(window, false);
        if (menu == IntPtr.Zero) {
            throw new InvalidOperationException("GetSystemMenu failed");
        }
        StringBuilder text = new StringBuilder(128);
        if (GetMenuStringW(menu, command, text, text.Capacity, 0) <= 0) {
            throw new InvalidOperationException(
                "system menu command was not found: " + command);
        }
        return text.ToString();
    }

    public static bool SystemMenuChecked(IntPtr window, uint command) {
        IntPtr menu = GetSystemMenu(window, false);
        if (menu == IntPtr.Zero) {
            throw new InvalidOperationException("GetSystemMenu failed");
        }
        return (GetMenuState(menu, command, 0) & 0x8) != 0;
    }

    public static void SystemCommand(IntPtr window, uint command) {
        SendMessageW(window, 0x0112, (UIntPtr)command, IntPtr.Zero);
    }

    public static IntPtr CreateForegroundHost() {
        IntPtr window = CreateWindowExW(
            0, "STATIC", "AgenTerm no-activate test host", 0x10CF0000,
            80, 80, 420, 160, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero);
        if (window == IntPtr.Zero) {
            throw new InvalidOperationException("CreateWindowExW test host failed");
        }
        ShowWindow(window, 5);
        SetForegroundWindow(window);
        return window;
    }

    public static IntPtr ForegroundWindow() {
        return GetForegroundWindow();
    }

    public static void DestroyHost(IntPtr window) {
        if (window != IntPtr.Zero) {
            DestroyWindow(window);
        }
    }

    public static void KeyDown(IntPtr window, byte virtualKey) {
        SetForegroundWindow(window);
        keybd_event(virtualKey, 0, 0, UIntPtr.Zero);
    }

    public static void KeyUp(byte virtualKey) {
        keybd_event(virtualKey, 0, 2, UIntPtr.Zero);
    }

    public static void ControlArrow(IntPtr window, byte virtualKey) {
        KeyDown(window, 0x11);
        KeyDown(window, virtualKey);
        KeyUp(virtualKey);
        KeyUp(0x11);
    }

    private static IntPtr FocusedControl(IntPtr window) {
        uint processId;
        uint threadId = GetWindowThreadProcessId(window, out processId);
        GUITHREADINFO info = new GUITHREADINFO();
        info.cbSize = (uint)Marshal.SizeOf<GUITHREADINFO>();
        if (threadId == 0 || !GetGUIThreadInfo(threadId, ref info)) {
            throw new InvalidOperationException("GetGUIThreadInfo failed");
        }
        return info.hwndFocus;
    }

    public static int[] EditSelection(IntPtr window) {
        IntPtr edit = FocusedControl(window);
        IntPtr start = Marshal.AllocHGlobal(4);
        IntPtr end = Marshal.AllocHGlobal(4);
        try {
            Marshal.WriteInt32(start, 0);
            Marshal.WriteInt32(end, 0);
            SendMessagePointer(edit, 0x00B0, start, end);
            return new int[] { Marshal.ReadInt32(start), Marshal.ReadInt32(end) };
        }
        finally {
            Marshal.FreeHGlobal(start);
            Marshal.FreeHGlobal(end);
        }
    }

    public static void SetEditSelection(IntPtr window, int start, int end) {
        IntPtr edit = FocusedControl(window);
        SendMessageW(edit, 0x00B1, (UIntPtr)(uint)start, (IntPtr)end);
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
    $localizedSendLabel = ([char]0x53D1).ToString() + [char]0x9001
    if ($snapshot.locale.id -ne 'en-US' -or
        $snapshot.locale.controls.send -ne 'Send' -or
        @($snapshot.locale.controls.PSObject.Properties.Value) -contains $localizedSendLabel) {
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

    Write-Host 'STEP truthful working-directory context and safe preparation'
    $snapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $cmdTab = $snapshot.tabs | Where-Object id -eq $id
    if ($cmdTab.working_context.cwd.source -ne 'launch' -or
        $cmdTab.working_context.cwd.pending -or
        [string]::IsNullOrWhiteSpace($cmdTab.working_context.cwd.path) -or
        $cmdTab.working_context.shell -ne 'cmd') {
        throw 'Initial cmd tab did not expose its truthful launch CWD and shell'
    }
    $cwdEditor = Invoke-AgenTerm @('ui-action', 'open-cwd-editor', '-t', $id) |
        ConvertFrom-Json
    if ($cwdEditor.modal.kind -ne 'cwd-editor' -or
        $cwdEditor.focus.surface -ne 'cwd-editor' -or
        $cwdEditor.layout.status_bar.cwd.action -ne 'open-cwd-editor' -or
        $cwdEditor.layout.status_bar.cwd.bounds.width -le 0) {
        throw 'CWD status segment/editor did not expose typed modal and focus semantics'
    }
    $cwdTargetWait = Invoke-AgenTerm @(
        'wait-ui', '--modal-target', $id, '--timeout-ms', '1000'
    ) | ConvertFrom-Json
    if ($cwdTargetWait.modal.kind -ne 'cwd-editor' -or
        $cwdTargetWait.modal.window_id -ne $id) {
        throw 'wait-ui --modal-target did not match the open CWD editor'
    }
    $cwdKindTargetWait = Invoke-AgenTerm @(
        'wait-ui', '--modal-kind', 'cwd-editor', '--modal-target', $name,
        '--timeout-ms', '1000'
    ) | ConvertFrom-Json
    if ($cwdKindTargetWait.modal.window_id -ne $id) {
        throw 'wait-ui did not resolve a named modal target to its stable tab ID'
    }
    Invoke-AgenTerm @('ui-action', 'cancel') | Out-Null
    Invoke-AgenTerm @(
        'wait-ui', '--modal-kind', 'none', '--timeout-ms', '1000'
    ) | Out-Null

    $previousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $modalTimeoutOutput = & $Exe wait-ui --modal-kind proxy-editor `
            --modal-target $id --timeout-ms 0 2>&1
        $modalTimeoutExitCode = $LASTEXITCODE
        $closedTargetOutput = & $Exe wait-ui --modal-kind closed `
            --modal-target $id --timeout-ms 0 2>&1
        $closedTargetExitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previousPreference
    }
    $modalTimeout = (($modalTimeoutOutput | ForEach-Object ToString) -join "`n") |
        ConvertFrom-Json
    if ($modalTimeoutExitCode -ne 1 -or
        $modalTimeout.code -ne 'ui_wait_timeout' -or
        $modalTimeout.timeout_ms -ne 0 -or
        $modalTimeout.expected.modal_kind -ne 'proxy-editor' -or
        $modalTimeout.expected.modal_target -ne $id) {
        throw 'wait-ui modal timeout did not expose its stable typed condition'
    }
    if ($closedTargetExitCode -ne 2 -or
        (($closedTargetOutput | ForEach-Object ToString) -join "`n") -notmatch
            'cannot be combined') {
        throw 'wait-ui accepted a contradictory closed-modal target condition'
    }

    $proxyEditor = Invoke-AgenTerm @(
        'ui-action', 'open-proxy-editor', '-t', $id
    ) | ConvertFrom-Json
    $proxyWait = Invoke-AgenTerm @(
        'wait-ui', '--modal-kind', 'proxy-editor', '--modal-target', $id,
        '--timeout-ms', '1000'
    ) | ConvertFrom-Json
    if ($proxyEditor.modal.kind -ne 'proxy-editor' -or
        $proxyWait.modal.window_id -ne $id) {
        throw 'wait-ui did not match the targeted proxy editor'
    }
    Invoke-AgenTerm @('ui-action', 'cancel') | Out-Null
    Invoke-AgenTerm @(
        'wait-ui', '--modal-kind', 'closed', '--timeout-ms', '1000'
    ) | Out-Null
    $inputBytesBefore = [int](Invoke-AgenTerm @(
        'list-panes', '-t', $id, '-F', '#{pane_input_bytes}'
    ))
    $protectedDraft = "echo AGENTERM_CWD_DRAFT_$PID"
    Invoke-AgenTerm @('set-composer', '-t', $id, $protectedDraft) | Out-Null
    $previousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $null = & $Exe ui-action cwd-prepare -t $id --path 'C:\Program Files' 2>&1
        $protectedExitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previousPreference
    }
    if ($protectedExitCode -eq 0 -or
        (Invoke-AgenTerm @('show-composer', '-t', $id)) -ne $protectedDraft) {
        throw 'Default CWD Prepare silently overwrote an existing Composer draft'
    }
    Invoke-AgenTerm @(
        'ui-action', 'cwd-prepare-append', '-t', $id,
        '--path', 'C:\Program Files'
    ) | Out-Null
    $preparedCmd = Invoke-AgenTerm @('show-composer', '-t', $id)
    if ($preparedCmd -notlike "$protectedDraft*cd /d `"C:\Program Files`"") {
        throw "cmd CWD preparation was not safely quoted and explicitly appended: [$preparedCmd]"
    }
    $inputBytesAfter = [int](Invoke-AgenTerm @(
        'list-panes', '-t', $id, '-F', '#{pane_input_bytes}'
    ))
    $snapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $cmdTab = $snapshot.tabs | Where-Object id -eq $id
    if ($inputBytesAfter -ne $inputBytesBefore -or
        $cmdTab.working_context.cwd.source -ne 'user_requested' -or
        -not $cmdTab.working_context.cwd.pending -or
        $cmdTab.working_context.cwd.confirmed_path -eq
            $cmdTab.working_context.cwd.path) {
        throw 'CWD Prepare sent input or falsely reported an unconfirmed request as applied'
    }

    $powershellId = Invoke-AgenTerm @(
        'new-window', '-d', '-n', "cwd-powershell-$PID", '-F', '#{window_id}',
        '--', 'powershell.exe', '-NoLogo', '-NoProfile'
    )
    Invoke-AgenTerm @(
        'wait-ui', '-t', $powershellId, '--tab-state', 'running', '--timeout-ms', '10000'
    ) | Out-Null
    Invoke-AgenTerm @(
        'ui-action', 'cwd-prepare-replace', '-t', $powershellId,
        '--path', "C:\O'Brien"
    ) | Out-Null
    $preparedPowerShell = Invoke-AgenTerm @('show-composer', '-t', $powershellId)
    if ($preparedPowerShell -ne "Set-Location -LiteralPath 'C:\O''Brien'") {
        throw "PowerShell CWD preparation was not literal-safe: [$preparedPowerShell]"
    }

    $validOscCommand =
        "[Console]::Write([char]27 + ']7;file:///C:/osc%20valid' + [char]7)"
    Invoke-AgenTerm @('set-composer', '-t', $powershellId, $validOscCommand) | Out-Null
    Invoke-AgenTerm @('send-composer', '-t', $powershellId) | Out-Null
    Invoke-AgenTerm @(
        'wait-pane', '-t', $powershellId, '--submit-complete', '--timeout-ms', '10000'
    ) | Out-Null
    $oscConfirmed = $false
    for ($attempt = 0; $attempt -lt 50; $attempt++) {
        $snapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
        $powershellTab = $snapshot.tabs | Where-Object id -eq $powershellId
        if ($powershellTab.working_context.cwd.source -eq 'osc7' -and
            $powershellTab.working_context.cwd.path -eq 'C:\osc valid' -and
            -not $powershellTab.working_context.cwd.pending) {
            $oscConfirmed = $true
            break
        }
        Start-Sleep -Milliseconds 50
    }
    if (-not $oscConfirmed) {
        throw 'Valid local OSC 7 did not confirm the last-known CWD'
    }
    $invalidOscCommand =
        "[Console]::Write([char]27 + ']7;file:///C:/bad%0dvalue' + [char]7)"
    Invoke-AgenTerm @('set-composer', '-t', $powershellId, $invalidOscCommand) | Out-Null
    Invoke-AgenTerm @('send-composer', '-t', $powershellId) | Out-Null
    Invoke-AgenTerm @(
        'wait-pane', '-t', $powershellId, '--submit-complete', '--timeout-ms', '10000'
    ) | Out-Null
    $snapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $powershellTab = $snapshot.tabs | Where-Object id -eq $powershellId
    if ($powershellTab.working_context.cwd.path -ne 'C:\osc valid' -or
        $powershellTab.working_context.cwd.source -ne 'osc7') {
        throw 'Malformed OSC 7 overwrote the last valid CWD'
    }

    $unknownId = Invoke-AgenTerm @(
        'new-window', '-d', '-n', "cwd-unknown-$PID", '-F', '#{window_id}',
        '--', "$env:SystemRoot\System32\where.exe", 'cmd'
    )
    $previousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $null = & $Exe ui-action cwd-send-now -t $unknownId --path 'C:\Windows' 2>&1
        $unknownSendExitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previousPreference
    }
    if ($unknownSendExitCode -eq 0) {
        throw 'CWD Send Now was available for an unknown shell'
    }
    Write-Evidence 'ux.working-context-cwd'

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

    Write-Host 'STEP no-activate launcher preserves foreground on existing server'
    $foregroundHost = [AgenTermNativeTest]::CreateForegroundHost()
    $foregroundWait = [Diagnostics.Stopwatch]::StartNew()
    while ([AgenTermNativeTest]::ForegroundWindow() -ne $foregroundHost -and
        $foregroundWait.ElapsedMilliseconds -lt 1000) {
        Start-Sleep -Milliseconds 10
    }
    if ([AgenTermNativeTest]::ForegroundWindow() -ne $foregroundHost) {
        throw 'could not establish the no-activate test host as foreground'
    }
    $noActivate = Start-Process -FilePath $GuiExe -ArgumentList @(
        '--no-activate', '--address', $env:AGENTERM_IPC_ADDRESS
    ) -PassThru
    if (-not $noActivate.WaitForExit(1000) -or
        [AgenTermNativeTest]::ForegroundWindow() -ne $foregroundHost) {
        throw '--no-activate existing-server handoff stole foreground or did not exit'
    }
    $noActivateSnapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    if (-not $noActivateSnapshot.window.visible -or
        $noActivateSnapshot.window.detached) {
        throw '--no-activate existing-server handoff lost the visible window'
    }

    $mainWorkspaceForNoActivate = $env:AGENTERM_WORKSPACE_PATH
    $mainSettingsForNoActivate = $env:AGENTERM_SETTINGS_PATH
    try {
        $env:AGENTERM_WORKSPACE_PATH = $noActivateWorkspaceFile
        $env:AGENTERM_SETTINGS_PATH = $noActivateSettingsFile
        $newNoActivate = Start-Process -FilePath $GuiExe -ArgumentList @(
            '--address', $noActivateAddress, '--not-foreground'
        ) -PassThru
    }
    finally {
        $env:AGENTERM_WORKSPACE_PATH = $mainWorkspaceForNoActivate
        $env:AGENTERM_SETTINGS_PATH = $mainSettingsForNoActivate
    }
    $newNoActivateWait = [Diagnostics.Stopwatch]::StartNew()
    $newNoActivateSnapshot = $null
    do {
        try {
            $newNoActivateSnapshot = Invoke-AgenTermAt -Address $noActivateAddress `
                -CommandArgs @('ui-snapshot') | ConvertFrom-Json
        }
        catch {
            if ($newNoActivateWait.ElapsedMilliseconds -ge 5000) { throw }
        }
    } while ($null -eq $newNoActivateSnapshot)
    if ([AgenTermNativeTest]::ForegroundWindow() -ne $foregroundHost -or
        -not $newNoActivateSnapshot.window.visible -or
        $newNoActivateSnapshot.window.detached) {
        throw '--no-activate alias new-server launch stole foreground or hid its window'
    }
    Invoke-AgenTermAt -Address $noActivateAddress -CommandArgs @('kill-server') | Out-Null
    $newNoActivate.WaitForExit(5000) | Out-Null
    [AgenTermNativeTest]::DestroyHost($foregroundHost)
    $foregroundHost = [IntPtr]::Zero
    Write-Evidence 'ux.no-activate-launch'

    Write-Host 'STEP adaptive Tabs controls, resizing, and shared layout'
    $tabsBaseline = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $baselineCols = [int]$tabsBaseline.layout.terminal.cols
    if ($tabsBaseline.layout.sidebar.configured_width -ne 250 -or
        $tabsBaseline.layout.sidebar.resize_grip.width -ne 6 -or
        -not $tabsBaseline.system_menu.toggle_tabs.checked -or
        [AgenTermNativeTest]::SystemMenuLabel($window, 0x1f20) -ne 'Toggle Tabs') {
        throw 'adaptive Tabs baseline, grip, or system menu was not exposed'
    }

    [AgenTermNativeTest]::ClickButton($window, 'Tabs')
    $tabsHidden = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    if ($tabsHidden.layout.sidebar.visible -or
        $tabsHidden.layout.sidebar.effective_width -ne 0 -or
        $tabsHidden.layout.terminal.x -ne 0 -or
        $null -eq $tabsHidden.layout.status_bar.tabs_recovery -or
        $tabsHidden.layout.terminal.cols -le $baselineCols -or
        $tabsHidden.system_menu.toggle_tabs.checked -or
        [AgenTermNativeTest]::SystemMenuChecked($window, 0x1f20)) {
        throw 'Tabs button did not hide the tree and release its terminal width'
    }
    $recovery = $tabsHidden.layout.status_bar.tabs_recovery
    [AgenTermNativeTest]::Click(
        $window,
        [int]($recovery.x + ($recovery.width / 2)),
        [int]($recovery.y + ($recovery.height / 2))
    )
    $tabsRecovered = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    if (-not $tabsRecovered.layout.sidebar.visible -or
        $tabsRecovered.layout.sidebar.effective_width -ne 250 -or
        $null -ne $tabsRecovered.layout.status_bar.tabs_recovery) {
        throw 'hidden status segment did not restore Tabs'
    }

    [AgenTermNativeTest]::SystemCommand($window, 0x1f20)
    $systemHidden = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    if ($systemHidden.layout.sidebar.visible -or
        $systemHidden.system_menu.toggle_tabs.checked) {
        throw 'Toggle Tabs system-menu command did not hide Tabs'
    }
    [AgenTermNativeTest]::SystemCommand($window, 0x1f20)
    $systemShown = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    if (-not $systemShown.layout.sidebar.visible -or
        -not $systemShown.system_menu.toggle_tabs.checked) {
        throw 'Toggle Tabs system-menu command did not restore checked state'
    }

    $grip = $systemShown.layout.sidebar.resize_grip
    [AgenTermNativeTest]::Drag(
        $window,
        [int]($grip.x + ($grip.width / 2)),
        [int]($grip.y + 80),
        330,
        [int]($grip.y + 80)
    )
    $tabsResized = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    if ($tabsResized.layout.sidebar.configured_width -ne 330 -or
        $tabsResized.layout.sidebar.effective_width -ne 330 -or
        $tabsResized.layout.terminal.cols -ge $baselineCols -or
        $tabsResized.layout.terminal.scrollbar.track.x -ne
            ($tabsResized.layout.terminal.x +
                $tabsResized.layout.terminal.width - 12)) {
        throw (
            'physical Tabs grip drag did not resize the shared terminal geometry: ' +
            ($tabsResized.layout | ConvertTo-Json -Depth 8 -Compress) +
            "; baseline_cols=$baselineCols"
        )
    }
    [AgenTermNativeTest]::DoubleClick(
        $window,
        [int]($tabsResized.layout.sidebar.resize_grip.x + 2),
        [int]($tabsResized.layout.sidebar.resize_grip.y + 80)
    )
    $tabsReset = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    if ($tabsReset.layout.sidebar.configured_width -ne 250 -or
        $tabsReset.layout.sidebar.effective_width -ne 250) {
        throw 'double-clicking the Tabs grip did not restore the 250px default'
    }
    Write-Evidence 'ux.adaptive-tabs'

    Write-Host 'STEP physical keyboard surface navigation and native Edit arbitration'
    Invoke-AgenTerm @('set-composer', '-t', $id, 'alpha beta gamma') | Out-Null
    Invoke-AgenTerm @('focus', 'composer', '-t', $id) | Out-Null
    [AgenTermNativeTest]::SetEditSelection($window, 16, 16)
    $composerBefore = [AgenTermNativeTest]::EditSelection($window)
    [AgenTermNativeTest]::ControlArrow($window, 0x25)
    $composerSnapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $composerAfter = [AgenTermNativeTest]::EditSelection($window)
    if ($composerSnapshot.focus.surface -ne 'composer' -or
        $composerAfter[0] -ge $composerBefore[0]) {
        throw 'Composer Ctrl+Left was stolen instead of reaching the native Edit control'
    }

    Invoke-AgenTerm @('focus', 'terminal', '-t', $id) | Out-Null
    [AgenTermNativeTest]::ControlArrow($window, 0x28)
    Invoke-AgenTerm @('wait-ui', '--active', $id, '--focus', 'composer') | Out-Null
    [AgenTermNativeTest]::ControlArrow($window, 0x26)
    Invoke-AgenTerm @('wait-ui', '--active', $id, '--focus', 'terminal') | Out-Null

    $hiddenTabs = Invoke-AgenTerm @('ui-action', 'toggle-tabs') | ConvertFrom-Json
    if ($hiddenTabs.layout.sidebar.visible -or
        $hiddenTabs.layout.sidebar.effective_width -ne 0) {
        throw 'toggle-tabs did not expose a hidden zero-width Tabs surface'
    }
    [AgenTermNativeTest]::ControlArrow($window, 0x25)
    Invoke-AgenTerm @(
        'wait-ui', '--active', $id, '--focus', 'tabs'
    ) | Out-Null
    $restoredTabs = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    if (-not $restoredTabs.layout.sidebar.visible -or
        $restoredTabs.layout.sidebar.effective_width -le 0) {
        throw 'Terminal Ctrl+Left did not restore and focus hidden Tabs'
    }
    [AgenTermNativeTest]::ControlArrow($window, 0x27)
    Invoke-AgenTerm @('wait-ui', '--active', $id, '--focus', 'terminal') | Out-Null

    $repeatBaseline = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    [AgenTermNativeTest]::KeyDown($window, 0x11)
    [AgenTermNativeTest]::KeyDown($window, 0x28)
    Invoke-AgenTerm @('wait-ui', '--active', $id, '--focus', 'composer') | Out-Null
    [AgenTermNativeTest]::KeyDown($window, 0x28)
    [AgenTermNativeTest]::KeyDown($window, 0x28)
    [AgenTermNativeTest]::KeyUp(0x28)
    [AgenTermNativeTest]::KeyUp(0x11)
    $repeatSnapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $repeatEvents = Invoke-AgenTerm @(
        'read-events',
        '--epoch', $repeatBaseline.event_position.epoch,
        '--after', "$($repeatBaseline.event_position.sequence)"
    ) | ConvertFrom-Json
    $keyboardFocusEvents = @(
        $repeatEvents.events |
            Where-Object {
                $_.kind -eq 'focus.changed' -and $_.payload.cause -eq 'keyboard'
            }
    )
    if ($repeatSnapshot.focus.surface -ne 'composer' -or
        $keyboardFocusEvents.Count -ne 1) {
        throw 'held Ctrl+Down crossed focus more than once'
    }

    $settingsSnapshot = Invoke-AgenTerm @('ui-action', 'open-settings') | ConvertFrom-Json
    if ($settingsSnapshot.focus.surface -ne 'settings') {
        throw 'Settings did not expose its native Edit focus'
    }
    [AgenTermNativeTest]::SetEditSelection($window, 7, 7)
    $settingsBefore = [AgenTermNativeTest]::EditSelection($window)
    [AgenTermNativeTest]::ControlArrow($window, 0x25)
    $settingsAfterSnapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $settingsAfter = [AgenTermNativeTest]::EditSelection($window)
    if ($settingsAfterSnapshot.focus.surface -ne 'settings' -or
        $settingsAfter[0] -ge $settingsBefore[0]) {
        throw 'Settings Ctrl+Left was stolen instead of reaching the native Edit control'
    }
    Invoke-AgenTerm @('ui-action', 'cancel') | Out-Null

    $noteSnapshot = Invoke-AgenTerm @('ui-action', 'edit-tab', '-t', $id) | ConvertFrom-Json
    if ($noteSnapshot.focus.surface -ne 'note-editor') {
        throw 'Tab note editor did not expose its native Edit focus'
    }
    [AgenTermNativeTest]::SetEditSelection($window, 7, 7)
    $noteBefore = [AgenTermNativeTest]::EditSelection($window)
    [AgenTermNativeTest]::ControlArrow($window, 0x25)
    $noteAfterSnapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $noteAfter = [AgenTermNativeTest]::EditSelection($window)
    if ($noteAfterSnapshot.focus.surface -ne 'note-editor' -or
        $noteAfter[0] -ge $noteBefore[0]) {
        throw 'Note editor Ctrl+Left was stolen instead of reaching the native Edit control'
    }
    Invoke-AgenTerm @('ui-action', 'cancel') | Out-Null
    Write-Evidence 'ux.keyboard-surface-navigation'

    if ([AgenTermNativeTest]::SystemMenuLabel($window, 0x1f00) -notlike 'Copy*' -or
        [AgenTermNativeTest]::SystemMenuLabel($window, 0x1f10) -notlike 'Paste*') {
        throw 'window icon system menu did not expose Copy and Paste'
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
    $clipboardWait = [Diagnostics.Stopwatch]::StartNew()
    do {
        $snapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
        if ($snapshot.system_menu.copy.enabled -and
            $snapshot.system_menu.paste.enabled) {
            break
        }
    } while ($clipboardWait.ElapsedMilliseconds -lt 1000)
    if (-not $snapshot.system_menu.copy.enabled -or
        -not $snapshot.system_menu.paste.enabled) {
        throw 'system-menu clipboard state was not enabled for terminal selection and text'
    }
    [AgenTermNativeTest]::SystemCommand($window, 0x1f00)
    $copyWait = [Diagnostics.Stopwatch]::StartNew()
    do {
        $copied = Get-Clipboard -Raw
        if ($copied -eq $selectionToken) {
            break
        }
    } while ($copyWait.ElapsedMilliseconds -lt 1000)
    if ($copied -ne $selectionToken) {
        throw "terminal selection copied unexpected text: '$copied'"
    }
    Write-Evidence 'ux.terminal-selection-copy'

    Write-Host 'STEP window icon system-menu terminal paste'
    $pasteCommand = "echo AGENTERM_SYSTEM_MENU_PASTE_$PID"
    Set-Clipboard -Value $pasteCommand
    [AgenTermNativeTest]::SystemCommand($window, 0x1f10)
    try {
        Invoke-AgenTerm @(
            'wait-pane', '-t', $id, '--contains', $pasteCommand, '--timeout-ms', '5000'
        ) | Out-Null
    }
    catch {
        $pasteFailureSnapshot = Invoke-AgenTerm @('ui-snapshot')
        $pasteFailureCapture = Invoke-AgenTerm @('capture-pane', '-p', '-t', $id)
        throw (
            "$($_.Exception.Message)`nPaste failure snapshot:`n$pasteFailureSnapshot" +
            "`nPaste failure capture:`n$pasteFailureCapture"
        )
    }
    $snapshot = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $tab = $snapshot.tabs | Where-Object id -eq $id
    if ($null -ne $tab.selection -or
        $snapshot.feedback.message -notmatch '^Pasted \d+ characters into @\d+$') {
        throw 'system-menu paste did not clear selection and report terminal delivery'
    }
    Invoke-AgenTerm @('send-keys', '-t', $id, 'Enter') | Out-Null
    Write-Evidence 'ux.system-menu-clipboard'

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

    Write-Host 'STEP window close cancel and detach preserve the live server'
    $continuityToken = "AGENTERM_DETACH_BEFORE_$PID"
    Invoke-AgenTerm @('send-keys', '-t', $id, "echo $continuityToken", 'Enter') | Out-Null
    Invoke-AgenTerm @(
        'wait-pane', '-t', $id, '--contains', $continuityToken, '--timeout-ms', '5000'
    ) | Out-Null
    $beforeWindowClose = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $beforePane = Get-PaneSnapshot -Target $id
    $beforeServer = Get-IsolatedServer
    $beforeTabIds = @($beforeWindowClose.tabs.id | Sort-Object)

    $closeModal = Invoke-AgenTerm @('ui-action', 'close-window') | ConvertFrom-Json
    if ($closeModal.modal.kind -ne 'confirm-window-close' -or
        $closeModal.modal.default_action -ne 'keep-server-running' -or
        (Compare-Object @(
            'keep-server-running', 'stop-server-and-exit', 'cancel'
        ) @($closeModal.modal.actions))) {
        throw 'window close did not expose the three-choice detach-first modal'
    }
    if (-not $closeModal.window.visible -or $closeModal.window.detached) {
        throw 'opening the window-close modal changed window visibility'
    }
    $closeModalWait = Invoke-AgenTerm @(
        'wait-ui', '--modal-kind', 'confirm-window-close', '--timeout-ms', '1000'
    ) | ConvertFrom-Json
    if ($closeModalWait.modal.default_action -ne 'keep-server-running') {
        throw 'wait-ui did not match the untargeted window-close confirmation'
    }

    $afterCancel = Invoke-AgenTerm @('ui-action', 'cancel') | ConvertFrom-Json
    Invoke-AgenTerm @(
        'wait-ui', '--modal-kind', 'none', '--timeout-ms', '1000'
    ) | Out-Null
    $afterCancelPane = Get-PaneSnapshot -Target $id
    $afterCancelServer = Get-IsolatedServer
    if ($null -ne $afterCancel.modal -or
        -not $afterCancel.window.visible -or $afterCancel.window.detached -or
        -not $afterCancel.layout.composer.visible -or
        -not $afterCancel.layout.composer.input_visible -or
        -not $afterCancel.layout.composer.send_visible -or
        $afterCancel.event_position.epoch -ne $beforeWindowClose.event_position.epoch -or
        $afterCancelPane.pid -ne $beforePane.pid -or
        $afterCancelServer.pid -ne $beforeServer.pid -or
        (Compare-Object $beforeTabIds @($afterCancel.tabs.id | Sort-Object))) {
        throw 'cancel changed server, epoch, tab, PTY, or visible-window identity'
    }

    $detachStart = Invoke-AgenTerm @('ui-action', 'close-window') | ConvertFrom-Json
    $detached = Invoke-AgenTerm @('ui-action', 'keep-server-running') | ConvertFrom-Json
    $detachEvent = Invoke-AgenTerm @(
        'wait-events',
        '--epoch', $detachStart.event_position.epoch,
        '--after', "$($detachStart.event_position.sequence)",
        '--kind', 'window.visibility',
        '--timeout-ms', '5000'
    ) | ConvertFrom-Json
    if ($detachEvent.payload.visible -ne $false -or
        $detachEvent.payload.reason -ne 'detach' -or
        $detached.window.visible -or -not $detached.window.detached -or
        $detached.layout.composer.visible) {
        throw 'keep-server-running did not publish and expose detached window state'
    }

    $hiddenToken = "AGENTERM_DETACH_HIDDEN_$PID"
    Invoke-AgenTerm @('send-keys', '-t', $id, "echo $hiddenToken", 'Enter') | Out-Null
    Invoke-AgenTerm @(
        'wait-pane', '-t', $id, '--contains', $hiddenToken, '--timeout-ms', '5000'
    ) | Out-Null
    $hiddenPane = Get-PaneSnapshot -Target $id
    $hiddenServer = Get-IsolatedServer
    $hiddenCapture = Invoke-AgenTerm @('capture-pane', '-p', '-t', $id)
    if ($hiddenPane.pid -ne $beforePane.pid -or
        $hiddenPane.output_bytes -le $beforePane.output_bytes -or
        $hiddenServer.pid -ne $beforeServer.pid -or
        $hiddenCapture -notmatch [regex]::Escape($continuityToken) -or
        $hiddenCapture -notmatch [regex]::Escape($hiddenToken)) {
        throw 'detached server did not preserve and advance the existing live PTY'
    }

    $reattachStart = $detached.event_position
    Start-Process -FilePath $GuiExe | Out-Null
    $reattachEvent = Invoke-AgenTerm @(
        'wait-events',
        '--epoch', $reattachStart.epoch,
        '--after', "$($reattachStart.sequence)",
        '--kind', 'window.visibility',
        '--timeout-ms', '5000'
    ) | ConvertFrom-Json
    $reattached = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    $reattachedPane = Get-PaneSnapshot -Target $id
    $reattachedServer = Get-IsolatedServer
    if ($reattachEvent.payload.visible -ne $true -or
        $reattachEvent.payload.reason -ne 'launcher' -or
        -not $reattached.window.visible -or $reattached.window.detached -or
        -not $reattached.layout.composer.visible -or
        -not $reattached.layout.composer.input_visible -or
        -not $reattached.layout.composer.send_visible -or
        $reattached.event_position.epoch -ne $beforeWindowClose.event_position.epoch -or
        $reattachedPane.pid -ne $beforePane.pid -or
        $reattachedServer.pid -ne $beforeServer.pid -or
        (Compare-Object $beforeTabIds @($reattached.tabs.id | Sort-Object))) {
        throw (
            'agenterm.exe reattach did not preserve server, epoch, tabs, PTY identity, ' +
            'and native composer controls'
        )
    }

    Write-Host 'STEP tab editor controls survive detach and reattach'
    $noteEditor = Invoke-AgenTerm @('ui-action', 'edit-tab', '-t', $id) | ConvertFrom-Json
    if (-not $noteEditor.layout.composer.visible -or
        $noteEditor.focus.surface -ne 'note-editor') {
        throw 'tab editor did not start with its native composer visible'
    }
    $noteDetachStart = Invoke-AgenTerm @('ui-action', 'close-window') | ConvertFrom-Json
    $noteDetached = Invoke-AgenTerm @(
        'ui-action', 'keep-server-running'
    ) | ConvertFrom-Json
    if ($noteDetached.layout.composer.visible -or
        -not $noteDetached.window.detached) {
        throw 'tab-editor detach did not hide the complete parent surface'
    }
    Start-Process -FilePath $GuiExe | Out-Null
    Invoke-AgenTerm @(
        'wait-events',
        '--epoch', $noteDetachStart.event_position.epoch,
        '--after', "$($noteDetachStart.event_position.sequence)",
        '--kind', 'window.visibility',
        '--timeout-ms', '5000'
    ) | Out-Null
    $noteReattached = Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
    if (-not $noteReattached.layout.composer.visible -or
        -not $noteReattached.layout.composer.input_visible -or
        -not $noteReattached.layout.composer.send_visible -or
        $noteReattached.focus.surface -ne 'note-editor') {
        throw 'reattached tab editor left its native input or Save control hidden'
    }
    Invoke-AgenTerm @('ui-action', 'cancel') | Out-Null

    Write-Host 'STEP stop server and exit creates a fresh isolated runtime'
    $mainWorkspace = $env:AGENTERM_WORKSPACE_PATH
    $mainSettings = $env:AGENTERM_SETTINGS_PATH
    try {
        $env:AGENTERM_WORKSPACE_PATH = $stopWorkspaceFile
        $env:AGENTERM_SETTINGS_PATH = $stopSettingsFile
        Invoke-AgenTermAt -Address $stopAddress -CommandArgs @('start-server') | Out-Null
    }
    finally {
        $env:AGENTERM_WORKSPACE_PATH = $mainWorkspace
        $env:AGENTERM_SETTINGS_PATH = $mainSettings
    }
    $stopName = "stop-runtime-$PID"
    Invoke-AgenTermAt -Address $stopAddress `
        -CommandArgs @('new-window', '-d', '-n', $stopName) | Out-Null
    $stopSnapshotBefore = Invoke-AgenTermAt -Address $stopAddress `
        -CommandArgs @('ui-snapshot') | ConvertFrom-Json
    $stopTabBefore = $stopSnapshotBefore.tabs | Where-Object name -eq $stopName
    if ($null -eq $stopTabBefore) {
        throw 'isolated stop-and-exit tab was not created'
    }
    Invoke-AgenTermAt -Address $stopAddress -CommandArgs @(
        'wait-ui', '-t', $stopTabBefore.id, '--tab-state', 'running', '--timeout-ms', '5000'
    ) | Out-Null
    $stopSnapshotBefore = Invoke-AgenTermAt -Address $stopAddress `
        -CommandArgs @('ui-snapshot') | ConvertFrom-Json
    $stopPaneBefore = Get-PaneSnapshot -Target $stopTabBefore.id -Address $stopAddress
    $stopServerBefore = Get-IsolatedServer -Address $stopAddress

    $stopModal = Invoke-AgenTermAt -Address $stopAddress `
        -CommandArgs @('ui-action', 'close-window') | ConvertFrom-Json
    if ($stopModal.modal.kind -ne 'confirm-window-close') {
        throw 'isolated server did not expose the stop-and-exit confirmation'
    }
    Invoke-AgenTermAt -Address $stopAddress `
        -CommandArgs @('ui-action', 'stop-server-and-exit') | Out-Null
    $stoppingServer = Get-Process -Id ([int]$stopServerBefore.pid) -ErrorAction SilentlyContinue
    if ($null -ne $stoppingServer) {
        $stoppingServer | Wait-Process -Timeout 10 -ErrorAction Stop
    }
    if (Get-Process -Id ([int]$stopServerBefore.pid) -ErrorAction SilentlyContinue) {
        throw 'stop-server-and-exit left the isolated server process running'
    }

    try {
        $env:AGENTERM_WORKSPACE_PATH = $stopWorkspaceFile
        $env:AGENTERM_SETTINGS_PATH = $stopSettingsFile
        Invoke-AgenTermAt -Address $stopAddress -CommandArgs @('start-server') | Out-Null
    }
    finally {
        $env:AGENTERM_WORKSPACE_PATH = $mainWorkspace
        $env:AGENTERM_SETTINGS_PATH = $mainSettings
    }
    Invoke-AgenTermAt -Address $stopAddress -CommandArgs @(
        'wait-ui', '-t', $stopTabBefore.id, '--tab-state', 'running', '--timeout-ms', '5000'
    ) | Out-Null
    $stopSnapshotAfter = Invoke-AgenTermAt -Address $stopAddress `
        -CommandArgs @('ui-snapshot') | ConvertFrom-Json
    $stopPaneAfter = Get-PaneSnapshot -Target $stopTabBefore.id -Address $stopAddress
    $stopServerAfter = Get-IsolatedServer -Address $stopAddress
    if ($stopSnapshotAfter.event_position.epoch -eq
            $stopSnapshotBefore.event_position.epoch -or
        $stopServerAfter.pid -eq $stopServerBefore.pid -or
        $stopPaneAfter.pid -eq $stopPaneBefore.pid) {
        throw 'restart after stop-and-exit reused the old server epoch or PTY'
    }
    Invoke-AgenTermAt -Address $stopAddress -CommandArgs @('kill-server') | Out-Null
    Write-Evidence 'ux.detach-first-window-close'

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
    $settingsWait = Invoke-AgenTerm @(
        'wait-ui', '--modal-kind', 'settings', '--timeout-ms', '1000'
    ) | ConvertFrom-Json
    if ($settingsWait.focus.surface -ne 'settings') {
        throw 'wait-ui did not match the Settings modal'
    }
    Invoke-AgenTerm @('ui-action', 'cancel') | Out-Null
    Invoke-AgenTerm @(
        'wait-ui', '--modal-kind', 'closed', '--timeout-ms', '1000'
    ) | Out-Null
    Write-Evidence 'ux.modal-wait'

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
    Invoke-AgenTerm @(
        'ui-action', 'cwd-prepare-append', '-t', $persistName,
        '--path', 'C:\restart-request-must-not-persist'
    ) | Out-Null
    Invoke-AgenTerm @('select-window', '-t', $persistName) | Out-Null
    Invoke-AgenTerm @('shutdown') | Out-Null
    for ($attempt = 0; $attempt -lt 50; $attempt++) {
        if (-not (Test-AgenTermReady)) { break }
        Start-Sleep -Milliseconds 50
    }
    Start-Process -FilePath $GuiExe | Out-Null
    $ready = $false
    for ($attempt = 0; $attempt -lt 100; $attempt++) {
        if (Test-AgenTermReady) {
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
        -not $restored.draft -or -not $restored.active -or
        $restored.working_context.cwd.source -ne 'launch' -or
        $restored.working_context.cwd.pending -or
        $restored.working_context.cwd.path -ne
            $restored.working_context.cwd.confirmed_path -or
        $restored.working_context.cwd.path -eq 'C:\restart-request-must-not-persist' -or
        $snapshot.layout.sidebar.configured_width -ne 250 -or
        -not $snapshot.layout.sidebar.visible) {
        throw 'Workspace metadata or adaptive Tabs settings did not survive restart'
    }
    Write-Evidence 'ux.persistent-workspace'
    Write-Host 'PASS: UX state, two-line tabs, settings, composer, focus, safe close, dead close, protocol discovery'
}
finally {
    [AgenTermNativeTest]::DestroyHost($foregroundHost)
    Remove-Item -LiteralPath $draftFile -ErrorAction SilentlyContinue
    try {
        & $Exe --address $stopAddress kill-server 2>$null | Out-Null
    }
    catch {
        # The isolated stop journey normally leaves no server to clean up.
    }
    try {
        & $Exe --address $noActivateAddress kill-server 2>$null | Out-Null
    }
    catch {
        # The isolated no-activate server normally stopped in its journey.
    }
    try {
        & $Exe kill-server 2>$null | Out-Null
    }
    catch {
        # Cleanup is best-effort when the main journey already stopped the server.
    }
    Remove-Item -LiteralPath $workspaceFile -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $settingsFile -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $stopWorkspaceFile -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $stopSettingsFile -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $noActivateWorkspaceFile -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $noActivateSettingsFile -ErrorAction SilentlyContinue
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
