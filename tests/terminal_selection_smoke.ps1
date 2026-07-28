param(
    [string]$Exe = (Join-Path $PSScriptRoot '..\dist\agenterm-cli.exe'),
    [switch]$ListEvidence
)

$ErrorActionPreference = 'Stop'
$declaredEvidence = @('ux.terminal-selection-professional')
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

function Get-UiSnapshot {
    return Invoke-AgenTerm @('ui-snapshot') | ConvertFrom-Json
}

function Get-PaneSnapshot {
    param([Parameter(Mandatory = $true)][string]$Target)
    $snapshot = Invoke-AgenTerm @('pane-snapshot', '-t', $Target) | ConvertFrom-Json
    $panes = @($snapshot.windows)
    if ($panes.Count -ne 1) {
        throw "pane-snapshot did not return exactly one pane for $Target"
    }
    return $panes[0]
}

function Assert-SelectionState {
    param(
        [Parameter(Mandatory = $true)]$Snapshot,
        [Parameter(Mandatory = $true)][string]$Phase,
        [bool]$AutoScroll
    )
    $interaction = $Snapshot.terminal_interaction.selection
    if ($interaction.phase -ne $Phase -or
        [bool]$interaction.autoscroll.active -ne $AutoScroll) {
        throw (
            "unexpected selection interaction: " +
            ($interaction | ConvertTo-Json -Compress -Depth 6)
        )
    }
    return $interaction
}

Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class AgenTermSelectionNativeTest {
    [DllImport("user32.dll")]
    private static extern IntPtr SendMessageW(
        IntPtr window, uint message, UIntPtr wParam, IntPtr lParam);

    private static IntPtr PointParam(int x, int y) {
        return (IntPtr)(((y & 0xffff) << 16) | (x & 0xffff));
    }

    public static void Down(IntPtr window, int x, int y) {
        SendMessageW(window, 0x0201, UIntPtr.Zero, PointParam(x, y));
    }

    public static void MoveHeld(IntPtr window, int x, int y) {
        SendMessageW(window, 0x0200, (UIntPtr)1, PointParam(x, y));
    }

    public static void Up(IntPtr window, int x, int y) {
        SendMessageW(window, 0x0202, UIntPtr.Zero, PointParam(x, y));
    }

    public static void Click(IntPtr window, int x, int y) {
        Down(window, x, y);
        Up(window, x, y);
    }

    public static void DoubleClick(IntPtr window, int x, int y) {
        SendMessageW(window, 0x0203, (UIntPtr)1, PointParam(x, y));
    }

    public static void CaptureChanged(IntPtr window) {
        SendMessageW(window, 0x0215, UIntPtr.Zero, IntPtr.Zero);
    }
}
'@

$previousAddress = $env:AGENTERM_IPC_ADDRESS
$previousWorkspace = $env:AGENTERM_WORKSPACE_PATH
$previousSettings = $env:AGENTERM_SETTINGS_PATH
$previousNoActivate = $env:AGENTERM_NO_ACTIVATE
$env:AGENTERM_IPC_ADDRESS = "127.0.0.1:$((51000 + ($PID % 1000)))"
$env:AGENTERM_WORKSPACE_PATH = Join-Path $env:TEMP "agenterm-selection-$PID.json"
$env:AGENTERM_SETTINGS_PATH = Join-Path $env:TEMP "agenterm-selection-settings-$PID.json"
$env:AGENTERM_NO_ACTIVATE = '1'
$workspacePath = $env:AGENTERM_WORKSPACE_PATH
$settingsPath = $env:AGENTERM_SETTINGS_PATH

try {
    $id = (
        Invoke-AgenTerm @(
            'new-window', '-d', '-n', "selection-smoke-$PID",
            '-F', '#{window_id}'
        )
    ).Trim()
    Invoke-AgenTerm @('select-window', '-t', $id) | Out-Null
    Invoke-AgenTerm @('wait-ui', '--active', $id, '--timeout-ms', '10000') | Out-Null

    # This one word deliberately combines Unicode and path punctuation. The
    # black-box assertion checks copied text rather than hard-coding a display
    # width because the active terminal width table owns cell geometry.
    $word = 'C:\工作\alpha.rs'
    Invoke-AgenTerm @('send-keys', '-l', '-t', $id, "echo $word") | Out-Null
    Invoke-AgenTerm @('send-keys', '-t', $id, 'Enter') | Out-Null
    Invoke-AgenTerm @(
        'wait-pane', '-t', $id, '--contains', $word, '--timeout-ms', '10000'
    ) | Out-Null

    $capture = Invoke-AgenTerm @('capture-pane', '-p', '-t', $id)
    $lines = @($capture -split "`n" | ForEach-Object { $_.TrimEnd("`r") })
    $row = -1
    for ($index = 0; $index -lt $lines.Count; $index++) {
        if ($lines[$index] -eq $word) {
            $row = $index
            break
        }
    }
    if ($row -lt 0) {
        throw 'could not locate the Unicode path word in the visible viewport'
    }

    $snapshot = Get-UiSnapshot
    $terminal = $snapshot.layout.terminal
    $cellWidth = [int][Math]::Floor($terminal.viewport_width / $terminal.cols)
    $cellHeight = [int][Math]::Floor($terminal.height / $terminal.rows)
    if ($cellWidth -le 0 -or $cellHeight -le 0) {
        throw 'ui-snapshot did not expose usable terminal-cell geometry'
    }
    $x = [int]$terminal.x + (6 * $cellWidth) + [Math]::Max(1, [int]($cellWidth / 2))
    $y = [int]$terminal.y + ($row * $cellHeight) + [Math]::Max(1, [int]($cellHeight / 2))
    $window = @(
        Get-Process agenterm -ErrorAction SilentlyContinue |
            Where-Object MainWindowTitle -eq $snapshot.window.title |
            Select-Object -First 1 -ExpandProperty MainWindowHandle
    )
    if ($window.Count -eq 0 -or [IntPtr]$window[0] -eq [IntPtr]::Zero) {
        throw 'could not resolve the isolated AgenTerm native window'
    }
    $window = [IntPtr]$window[0]

    Write-Host 'STEP double-click Unicode/path word and explicit button-up'
    [AgenTermSelectionNativeTest]::DoubleClick($window, $x, $y)
    # Win32 sends LBUTTONUP after DBLCLK. This must not clear the completed word.
    [AgenTermSelectionNativeTest]::Up($window, $x, $y)
    $snapshot = Get-UiSnapshot
    $interaction = Assert-SelectionState -Snapshot $snapshot -Phase 'completed' `
        -AutoScroll $false
    if ($interaction.selection.start.row -ne $row -or
        $interaction.selection.start.col -ne 0 -or
        $interaction.selection.end.row -ne $row -or
        $interaction.selection.end.col -lt 10) {
        throw (
            "double-click did not select the full Unicode/path word: " +
            ($interaction.selection | ConvertTo-Json -Compress)
        )
    }
    Invoke-AgenTerm @('ui-action', 'copy-selection') | Out-Null
    $copiedWord = Get-Clipboard -Raw
    if ($copiedWord -ne $word) {
        throw "double-click copied '$copiedWord' instead of '$word'"
    }

    Write-Host 'STEP same-cell third click selects the visible row'
    [AgenTermSelectionNativeTest]::Down($window, $x, $y)
    [AgenTermSelectionNativeTest]::Up($window, $x, $y)
    $snapshot = Get-UiSnapshot
    $interaction = Assert-SelectionState -Snapshot $snapshot -Phase 'completed' `
        -AutoScroll $false
    if ($interaction.selection.start.row -ne $row -or
        $interaction.selection.start.col -ne 0 -or
        $interaction.selection.end.row -ne $row -or
        $interaction.selection.end.col -ne ([int]$terminal.cols - 1)) {
        throw 'same-cell third click did not select the full visible row'
    }

    Write-Host 'STEP ordinary unmoved click reaches the native terminal path'
    $beforeClick = Get-PaneSnapshot -Target $id
    [AgenTermSelectionNativeTest]::Click($window, $x, $y)
    $clickWait = [Diagnostics.Stopwatch]::StartNew()
    do {
        $afterClick = Get-PaneSnapshot -Target $id
        if ([int64]$afterClick.input_bytes -ge ([int64]$beforeClick.input_bytes + 3)) {
            break
        }
    } while ($clickWait.ElapsedMilliseconds -lt 2000)
    if ([int64]$afterClick.input_bytes -lt ([int64]$beforeClick.input_bytes + 3)) {
        throw 'ordinary unmoved click did not reach RMUX/native terminal input'
    }

    Write-Host 'STEP held drag outside viewport drives bounded timer autoscroll'
    $scrollPrefix = "AGENTERM_SELECTION_SCROLL_$PID"
    Invoke-AgenTerm @(
        'send-keys', '-l', '-t', $id,
        "for /L %i in (1,1,100) do @echo $scrollPrefix`_%i"
    ) | Out-Null
    Invoke-AgenTerm @('send-keys', '-t', $id, 'Enter') | Out-Null
    Invoke-AgenTerm @(
        'wait-pane', '-t', $id, '--contains', "$scrollPrefix`_100",
        '--timeout-ms', '10000'
    ) | Out-Null
    $snapshot = Get-UiSnapshot
    $terminal = $snapshot.layout.terminal
    $startX = [int]$terminal.x + (4 * $cellWidth)
    $startY = [int]$terminal.y + ([int]($terminal.rows / 2) * $cellHeight)
    [AgenTermSelectionNativeTest]::Down($window, $startX, $startY)
    [AgenTermSelectionNativeTest]::MoveHeld(
        $window, $startX, ([int]$terminal.y - (2 * $cellHeight))
    )
    $scrollWait = [Diagnostics.Stopwatch]::StartNew()
    do {
        $snapshot = Get-UiSnapshot
        $tab = $snapshot.tabs | Where-Object id -eq $id
        $interaction = $snapshot.terminal_interaction.selection
        if ($interaction.autoscroll.active -and [int]$tab.scrollback_offset -gt 0) {
            break
        }
    } while ($scrollWait.ElapsedMilliseconds -lt 3000)
    if (-not $interaction.autoscroll.active -or [int]$tab.scrollback_offset -le 0 -or
        [int]$interaction.autoscroll.rows_per_tick -lt 1 -or
        [int]$interaction.autoscroll.rows_per_tick -gt 3) {
        throw 'outside-viewport drag did not expose bounded timer autoscroll'
    }
    [AgenTermSelectionNativeTest]::Up($window, $startX, $startY)
    $snapshot = Get-UiSnapshot
    Assert-SelectionState -Snapshot $snapshot -Phase 'completed' `
        -AutoScroll $false | Out-Null

    Write-Host 'STEP capture loss cancels an unfinished drag'
    [AgenTermSelectionNativeTest]::Down($window, $startX, $startY)
    [AgenTermSelectionNativeTest]::MoveHeld(
        $window, ($startX + (3 * $cellWidth)), $startY
    )
    [AgenTermSelectionNativeTest]::CaptureChanged($window)
    $snapshot = Get-UiSnapshot
    $interaction = Assert-SelectionState -Snapshot $snapshot -Phase 'cancelled' `
        -AutoScroll $false
    $tab = $snapshot.tabs | Where-Object id -eq $id
    if ($null -ne $interaction.selection -or $null -ne $tab.selection) {
        throw 'capture loss left a partial terminal selection behind'
    }

    Write-Host 'STEP tab change cancels an unfinished drag'
    [AgenTermSelectionNativeTest]::Down($window, $startX, $startY)
    [AgenTermSelectionNativeTest]::MoveHeld(
        $window, ($startX + (2 * $cellWidth)), $startY
    )
    $other = (
        Invoke-AgenTerm @(
            'new-window', '-d', '-n', "selection-other-$PID",
            '-F', '#{window_id}'
        )
    ).Trim()
    Invoke-AgenTerm @('select-window', '-t', $other) | Out-Null
    Invoke-AgenTerm @('wait-ui', '--active', $other, '--timeout-ms', '5000') | Out-Null
    $snapshot = Get-UiSnapshot
    Assert-SelectionState -Snapshot $snapshot -Phase 'cancelled' `
        -AutoScroll $false | Out-Null

    Write-Host 'EVIDENCE ux.terminal-selection-professional'
    Write-Host 'PASS: professional terminal-selection physical-message contract'
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
