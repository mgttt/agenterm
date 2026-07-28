param(
    [string]$Exe = (Join-Path $PSScriptRoot '..\dist\agenterm-cli.exe'),
    [switch]$ListEvidence
)

$ErrorActionPreference = 'Stop'
$declaredEvidence = @(
    'ux.workbench-inline-edit'
    'ux.workbench-compact-tree'
)
if ($ListEvidence) {
    $declaredEvidence
    exit 0
}

$Exe = [IO.Path]::GetFullPath($Exe)
if (-not (Test-Path -LiteralPath $Exe)) {
    throw "AgenTerm executable not found: $Exe"
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
        [int]($Bounds.x + [Math]::Max(1, $Bounds.width / 2)),
        [int]($Bounds.y + [Math]::Max(1, $Bounds.height / 2))
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
        if ($matches -eq $Open) {
            return $snapshot
        }
    } while ($timer.ElapsedMilliseconds -lt 2000)
    throw "inline editor state did not become open=$Open for $Target"
}

function Assert-Inside {
    param(
        [Parameter(Mandatory = $true)]$Outer,
        [Parameter(Mandatory = $true)]$Inner,
        [Parameter(Mandatory = $true)][string]$Label
    )
    if ($Inner.x -lt $Outer.x -or $Inner.y -lt $Outer.y -or
        ($Inner.x + $Inner.width) -gt ($Outer.x + $Outer.width) -or
        ($Inner.y + $Inner.height) -gt ($Outer.y + $Outer.height) -or
        $Inner.width -lt 0 -or $Inner.height -lt 0) {
        throw "$Label escaped its row: $($Inner | ConvertTo-Json -Compress)"
    }
}

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class WorkbenchNativeTest {
    [DllImport("user32.dll")]
    private static extern IntPtr SendMessageW(
        IntPtr window, uint message, UIntPtr wParam, IntPtr lParam);

    private static IntPtr PointParam(int x, int y) {
        return (IntPtr)(((y & 0xffff) << 16) | (x & 0xffff));
    }

    public static void Click(IntPtr window, int x, int y) {
        IntPtr point = PointParam(x, y);
        SendMessageW(window, 0x0201, UIntPtr.Zero, point);
        SendMessageW(window, 0x0202, UIntPtr.Zero, point);
    }
}
'@

$previousAddress = $env:AGENTERM_IPC_ADDRESS
$previousWorkspace = $env:AGENTERM_WORKSPACE_PATH
$previousSettings = $env:AGENTERM_SETTINGS_PATH
$previousNoActivate = $env:AGENTERM_NO_ACTIVATE
$env:AGENTERM_IPC_ADDRESS = "127.0.0.1:$((52000 + ($PID % 1000)))"
$env:AGENTERM_WORKSPACE_PATH = Join-Path $env:TEMP "agenterm-workbench-$PID.json"
$env:AGENTERM_SETTINGS_PATH = Join-Path $env:TEMP "agenterm-workbench-settings-$PID.json"
$env:AGENTERM_NO_ACTIVATE = '1'
$workspacePath = $env:AGENTERM_WORKSPACE_PATH
$settingsPath = $env:AGENTERM_SETTINGS_PATH

try {
    $root = (
        Invoke-AgenTerm @(
            'new-window', '-d', '-n', "workbench-$PID", '-F', '#{window_id}'
        )
    ).Trim()
    Invoke-AgenTerm @('select-window', '-t', $root) | Out-Null
    Invoke-AgenTerm @('wait-ui', '--active', $root, '--timeout-ms', '10000') | Out-Null
    $snapshot = Get-Snapshot
    $window = @(
        Get-Process agenterm -ErrorAction SilentlyContinue |
            Where-Object MainWindowTitle -eq $snapshot.window.title |
            Select-Object -First 1 -ExpandProperty MainWindowHandle
    )
    if ($window.Count -eq 0 -or [IntPtr]$window[0] -eq [IntPtr]::Zero) {
        throw 'could not resolve the isolated AgenTerm native window'
    }
    $window = [IntPtr]$window[0]

    Write-Host 'STEP mouse Edit opens independent controls and mouse Save restores Edit'
    $rootTab = $snapshot.tabs | Where-Object id -eq $root
    if ($rootTab.actions.edit.label -ne 'Edit') {
        throw 'active normal row did not expose Edit'
    }
    Click-Bounds -Window $window -Bounds $rootTab.actions.edit.bounds
    $snapshot = Wait-Editor -Target $root -Open $true
    $composerBefore = Invoke-AgenTerm @('show-composer', '-t', $root)
    Invoke-AgenTerm @(
        'set-composer', '-t', $root, "工作台-$PID`ninline note"
    ) | Out-Null
    $draftSnapshot = Get-Snapshot
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
            "name='$($rootTab.name)' note='$($rootTab.note)' " +
            "action='$($rootTab.actions.edit.label)' " +
            "composer_before='$composerBefore' composer_after='$composerAfter'"
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
    Write-Host 'EVIDENCE ux.workbench-inline-edit'

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
        $expectedDensity = if ($width -eq 180) { 'compact' } else { 'full' }
        if ($tab.actions.density -ne $expectedDensity -or
            $tab.render.node.x -lt $tab.bounds.x -or
            $tab.render.name.x -ge $tab.actions.new_child.bounds.x) {
            throw "tree geometry was inconsistent at Tabs width $width"
        }
    }
    Write-Host 'EVIDENCE ux.workbench-compact-tree'
    Write-Host 'PASS: inline Tabs editing and compact tree geometry'
}
finally {
    try {
        & $Exe kill-server 2>$null | Out-Null
    }
    catch {
        # The isolated server may already have exited after a failed assertion.
    }
    Remove-Item -LiteralPath $workspacePath, $settingsPath -ErrorAction SilentlyContinue
    $env:AGENTERM_IPC_ADDRESS = $previousAddress
    $env:AGENTERM_WORKSPACE_PATH = $previousWorkspace
    $env:AGENTERM_SETTINGS_PATH = $previousSettings
    $env:AGENTERM_NO_ACTIVATE = $previousNoActivate
}
