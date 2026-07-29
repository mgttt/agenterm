param(
    [string]$Exe = (Join-Path $PSScriptRoot '..\dist\agenterm-cli.exe'),
    [string]$GuiExe = (Join-Path $PSScriptRoot '..\dist\agenterm.exe'),
    [switch]$ListEvidence
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'TestHarness.ps1')
$declaredEvidence = @(
    'ux.workbench-inline-edit'
    'ux.workbench-compact-tree'
    'ux.workbench-proxy-archived'
)
if ($ListEvidence) {
    $declaredEvidence
    exit 0
}

function Write-Evidence {
    param([Parameter(Mandatory = $true)][string]$Id)
    if ($declaredEvidence -notcontains $Id) {
        throw "workbench smoke emitted undeclared evidence ID: $Id"
    }
    Write-Host "EVIDENCE $Id"
}

$Exe = [IO.Path]::GetFullPath($Exe)
$GuiExe = [IO.Path]::GetFullPath($GuiExe)
foreach ($path in @($Exe, $GuiExe)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "AgenTerm executable not found: $path"
    }
}

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

function Get-Snapshot {
    Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
}

function Click-Bounds {
    param(
        [Parameter(Mandatory = $true)][IntPtr]$Window,
        [Parameter(Mandatory = $true)]$Bounds
    )
    [WorkbenchNativeTest]::Click(
        $Window,
        [int]($Bounds.left + [Math]::Max(1, $Bounds.width / 2)),
        [int]($Bounds.top + [Math]::Max(1, $Bounds.height / 2))
    )
}

function Wait-Editor {
    param(
        [Parameter(Mandatory = $true)][string]$Target,
        [Parameter(Mandatory = $true)][bool]$Open
    )
    $timer = [Diagnostics.Stopwatch]::StartNew()
    do {
        $snapshot = Get-Snapshot
        $matches = $null -ne $snapshot.tab_editor -and
            $snapshot.tab_editor.target -eq $Target
        $targetTab = @($snapshot.tabs | Where-Object id -eq $Target)[0]
        if ($snapshot.projection -eq 'replaceable_ui_client' -and
            $null -ne $targetTab -and $matches -eq $Open) {
            return $snapshot
        }
    } while ($timer.ElapsedMilliseconds -lt 2000)
    throw (
        "inline editor state did not become open=$Open for $Target`: " +
        "projection=$($snapshot.projection) " +
        "editor=$($snapshot.tab_editor | ConvertTo-Json -Depth 6 -Compress) " +
        "row=$($targetTab | ConvertTo-Json -Depth 8 -Compress)"
    )
}

function Wait-EditorDraft {
    param(
        [Parameter(Mandatory = $true)][string]$Target,
        [Parameter(Mandatory = $true)][int]$NameLength,
        [Parameter(Mandatory = $true)][int]$NoteLength
    )
    $timer = [Diagnostics.Stopwatch]::StartNew()
    do {
        $snapshot = Get-Snapshot
        if ($snapshot.tab_editor.target -eq $Target -and
            $snapshot.tab_editor.name_length -eq $NameLength -and
            $snapshot.tab_editor.note_length -eq $NoteLength) {
            return $snapshot
        }
        Start-Sleep -Milliseconds 10
    } while ($timer.ElapsedMilliseconds -lt 2000)
    throw (
        "inline editor draft did not become observable for $Target`: " +
        ($snapshot.tab_editor | ConvertTo-Json -Depth 6 -Compress)
    )
}

function Assert-Inside {
    param(
        [Parameter(Mandatory = $true)]$Outer,
        [Parameter(Mandatory = $true)]$Inner,
        [Parameter(Mandatory = $true)][string]$Label
    )
    if ($Inner.left -lt $Outer.left -or $Inner.top -lt $Outer.top -or
        $Inner.right -gt $Outer.right -or
        $Inner.bottom -gt $Outer.bottom -or
        $Inner.width -lt 0 -or $Inner.height -lt 0) {
        throw "$Label escaped its row: $($Inner | ConvertTo-Json -Compress)"
    }
}

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class WorkbenchNativeTest {
    [StructLayout(LayoutKind.Sequential)]
    private struct Point {
        public int X;
        public int Y;
    }

    [DllImport("user32.dll")]
    private static extern IntPtr SendMessageW(
        IntPtr window, uint message, UIntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    private static extern IntPtr ChildWindowFromPointEx(
        IntPtr parent, Point point, uint flags);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern IntPtr FindWindowExW(
        IntPtr parent, IntPtr after, string className, string title);

    private static IntPtr PointParam(int x, int y) {
        return (IntPtr)(((y & 0xffff) << 16) | (x & 0xffff));
    }

    public static void Click(IntPtr window, int x, int y) {
        Point location = new Point { X = x, Y = y };
        IntPtr child = ChildWindowFromPointEx(window, location, 0x0001 | 0x0002);
        if (child != IntPtr.Zero && child != window) {
            SendMessageW(child, 0x00F5, UIntPtr.Zero, IntPtr.Zero);
            return;
        }
        IntPtr packed = PointParam(x, y);
        SendMessageW(window, 0x0201, UIntPtr.Zero, packed);
        SendMessageW(window, 0x0202, UIntPtr.Zero, packed);
    }

    public static void ClickButton(IntPtr window, string label) {
        IntPtr button = FindWindowExW(window, IntPtr.Zero, "Button", label);
        if (button == IntPtr.Zero) {
            throw new InvalidOperationException("button not found: " + label);
        }
        SendMessageW(button, 0x00F5, UIntPtr.Zero, IntPtr.Zero);
    }

}
'@

$previousAddress = $env:AGENTERM_IPC_ADDRESS
$previousWorkspace = $env:AGENTERM_WORKSPACE_PATH
$previousSettings = $env:AGENTERM_SETTINGS_PATH
$previousNoActivate = $env:AGENTERM_NO_ACTIVATE
$previousHttpProxy = $env:HTTP_PROXY
$previousHttpsProxy = $env:HTTPS_PROXY
$previousHttpProxyLower = $env:http_proxy
$previousHttpsProxyLower = $env:https_proxy
$env:AGENTERM_IPC_ADDRESS = Get-SmokeLoopbackAddress
$env:AGENTERM_WORKSPACE_PATH = Join-Path $env:TEMP "agenterm-workbench-$PID.json"
$env:AGENTERM_SETTINGS_PATH = Join-Path $env:TEMP "agenterm-workbench-settings-$PID.json"
$env:AGENTERM_NO_ACTIVATE = '1'
Remove-Item Env:HTTP_PROXY, Env:HTTPS_PROXY, Env:http_proxy, Env:https_proxy `
    -ErrorAction SilentlyContinue
$workspacePath = $env:AGENTERM_WORKSPACE_PATH
$settingsPath = $env:AGENTERM_SETTINGS_PATH
$stderrPath = Join-Path $env:TEMP "agenterm-workbench-stderr-$PID.txt"
$guiProcess = $null

try {
    $guiProcess = Start-Process -FilePath $GuiExe -ArgumentList @(
        '--no-activate', '--address', $env:AGENTERM_IPC_ADDRESS
    ) -RedirectStandardError $stderrPath -PassThru
    if (-not $guiProcess.WaitForInputIdle(10000)) {
        throw 'isolated workbench GUI did not become input-idle within 10 seconds'
    }
    Invoke-AgenTerm @(
        'wait-ui', '--window-state', 'restored', '--timeout-ms', '10000'
    ) | Out-Null

    $root = (
        Invoke-AgenTerm @(
            'new-window', '-d', '-n', "workbench-$PID", '-F', '#{window_id}'
        )
    ).Trim()
    Invoke-AgenTerm @('select-window', '-t', $root) | Out-Null
    Invoke-AgenTerm @(
        'wait-ui', '--active', $root, '--window-state', 'restored',
        '--timeout-ms', '10000'
    ) | Out-Null
    $snapshot = Get-Snapshot
    $guiProcess.Refresh()
    $window = [IntPtr]$guiProcess.MainWindowHandle
    if ($window -eq [IntPtr]::Zero) {
        throw 'could not resolve the isolated AgenTerm native window'
    }

    Write-Host 'STEP mouse Edit opens independent controls and mouse Save restores Edit'
    $rootTab = $snapshot.tabs | Where-Object id -eq $root
    if ($rootTab.actions.edit.label -ne 'Edit') {
        $guiProcess.Refresh()
        $guiState = if ($guiProcess.HasExited) {
            "exited:$($guiProcess.ExitCode)"
        } else {
            "running:$($guiProcess.Id)"
        }
        $stderr = Get-Content -LiteralPath $stderrPath -Raw `
            -ErrorAction SilentlyContinue
        throw (
            'active normal row did not expose Edit: ' +
            "target=$root projection=$($snapshot.projection) gui=$guiState " +
            "row=$($rootTab | ConvertTo-Json -Depth 8 -Compress) " +
            "stderr=$stderr"
        )
    }
    Click-Bounds -Window $window -Bounds $rootTab.actions.edit.bounds
    $snapshot = Wait-Editor -Target $root -Open $true
    $composerBefore = Invoke-AgenTerm @('show-composer', '-t', $root)
    Invoke-AgenTerm @(
        'set-composer', '-t', $root, "工作台-$PID`ninline note"
    ) | Out-Null
    $draftSnapshot = Wait-EditorDraft -Target $root `
        -NameLength "工作台-$PID".Length -NoteLength 'inline note'.Length
    if ($draftSnapshot.tab_editor.name_length -ne "工作台-$PID".Length -or
        $draftSnapshot.tab_editor.note_length -ne 'inline note'.Length) {
        throw (
            'native inline editor text did not reach the host: ' +
            ($draftSnapshot.tab_editor | ConvertTo-Json -Compress)
        )
    }
    $rootTab = $snapshot.tabs | Where-Object id -eq $root
    Click-Bounds -Window $window -Bounds $rootTab.actions.save.bounds
    $snapshot = Wait-Editor -Target $root -Open $false
    $rootTab = $snapshot.tabs | Where-Object id -eq $root
    $composerAfter = Invoke-AgenTerm @('show-composer', '-t', $root)
    if ($rootTab.name -ne "工作台-$PID" -or $rootTab.note -ne 'inline note' -or
        $rootTab.actions.edit.label -ne 'Edit' -or $composerAfter -ne $composerBefore) {
        throw (
            'mouse Save did not atomically persist the row or preserve Composer: ' +
            "projection='$($snapshot.projection)' " +
            "event='$($snapshot.event_position | ConvertTo-Json -Compress)' " +
            "name='$($rootTab.name)' note='$($rootTab.note)' " +
            "action='$($rootTab.actions.edit.label)' " +
            "composer_before='$composerBefore' composer_after='$composerAfter' " +
            "row='$($rootTab | ConvertTo-Json -Depth 10 -Compress)'"
        )
    }

    Write-Host 'STEP mouse Cancel discards both independent drafts'
    Click-Bounds -Window $window -Bounds $rootTab.actions.edit.bounds
    $snapshot = Wait-Editor -Target $root -Open $true
    Invoke-AgenTerm @(
        'set-composer', '-t', $root, "discarded name`ndiscarded note"
    ) | Out-Null
    $rootTab = $snapshot.tabs | Where-Object id -eq $root
    Click-Bounds -Window $window -Bounds $rootTab.actions.cancel.bounds
    $snapshot = Wait-Editor -Target $root -Open $false
    $rootTab = $snapshot.tabs | Where-Object id -eq $root
    if ($rootTab.name -ne "工作台-$PID" -or $rootTab.note -ne 'inline note') {
        throw 'mouse Cancel persisted an inline draft'
    }
    Write-Evidence 'ux.workbench-inline-edit'

    Write-Host 'STEP archived Proxy status surface releases its layout and actions'
    $snapshot = Get-Snapshot
    $proxyStatus = $snapshot.layout.status_bar.proxy
    if (-not $proxyStatus.archived -or $proxyStatus.available -or
        $proxyStatus.bounds.width -ne 0 -or $null -ne $proxyStatus.action -or
        $null -ne $proxyStatus.eye_action) {
        throw 'Proxy status surface was not truthfully archived'
    }
    Write-Evidence 'ux.workbench-proxy-archived'

    Write-Host 'STEP compact and full geometry stay bounded for a deep CJK row'
    $target = $root
    foreach ($depth in 1..4) {
        $snapshot = Invoke-AgenTerm @('ui-action', 'new-child', '-t', $target) |
            ConvertFrom-Json
        $child = $snapshot.tabs | Where-Object active
        Invoke-AgenTerm @(
            'set-composer', '-t', $child.id, "层级-$depth-工作`n备注-$depth"
        ) | Out-Null
        Click-Bounds -Window $window -Bounds $child.actions.save.bounds
        $null = Wait-Editor -Target $child.id -Open $false
        $target = $child.id
    }

    foreach ($width in 180, 250, 480) {
        Invoke-AgenTerm @(
            'ui-action', 'tabs-set-width', '--width', "$width"
        ) | Out-Null
        $snapshot = Get-Snapshot
        $tab = $snapshot.tabs | Where-Object id -eq $target
        Assert-Inside -Outer $tab.bounds -Inner $tab.render.name `
            -Label "name at Tabs width $width"
        Assert-Inside -Outer $tab.bounds -Inner $tab.render.note `
            -Label "note at Tabs width $width"
        Assert-Inside -Outer $tab.bounds -Inner $tab.actions.edit.bounds `
            -Label "Edit at Tabs width $width"
        Assert-Inside -Outer $tab.bounds -Inner $tab.actions.close.bounds `
            -Label "Close at Tabs width $width"
        $expectedDensity = if ($width -lt 300) { 'compact' } else { 'full' }
        if ($tab.actions.density -ne $expectedDensity -or
            $tab.render.node.x -lt $tab.bounds.left -or
            $tab.render.name.left -ge $tab.actions.new_child.bounds.left) {
            throw (
                "tree geometry was inconsistent at Tabs width $width`: " +
                "expected_density=$expectedDensity " +
                ($tab | ConvertTo-Json -Depth 10 -Compress)
            )
        }
    }
    Write-Evidence 'ux.workbench-compact-tree'
    Write-Host 'PASS: inline Tabs editing and compact tree geometry'
}
finally {
    try {
        & $Exe kill-server 2>$null | Out-Null
    }
    catch {
        # The isolated server may already have exited after a failed assertion.
    }
    if ($null -ne $guiProcess -and -not $guiProcess.HasExited) {
        if (-not $guiProcess.WaitForExit(3000)) {
            Stop-Process -Id $guiProcess.Id -Force -ErrorAction SilentlyContinue
        }
    }
    Remove-Item -LiteralPath $workspacePath, $settingsPath, $stderrPath `
        -ErrorAction SilentlyContinue
    $env:AGENTERM_IPC_ADDRESS = $previousAddress
    $env:AGENTERM_WORKSPACE_PATH = $previousWorkspace
    $env:AGENTERM_SETTINGS_PATH = $previousSettings
    $env:AGENTERM_NO_ACTIVATE = $previousNoActivate
    $env:HTTP_PROXY = $previousHttpProxy
    $env:HTTPS_PROXY = $previousHttpsProxy
    $env:http_proxy = $previousHttpProxyLower
    $env:https_proxy = $previousHttpsProxyLower
}
