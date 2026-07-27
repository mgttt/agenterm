param(
    [string]$Exe = (Join-Path $PSScriptRoot '..\dist\agentermctl.exe')
)

$ErrorActionPreference = 'Stop'
$Exe = [IO.Path]::GetFullPath($Exe)
$GuiExe = Join-Path (Split-Path -Parent $Exe) 'agenterm.exe'
if (-not (Test-Path -LiteralPath $Exe)) {
    throw "AgenTerm executable not found: $Exe"
}
if (-not (Test-Path -LiteralPath $GuiExe)) {
    throw "AgenTerm GUI executable not found: $GuiExe"
}

$previousAddress = $env:AGENTERM_IPC_ADDRESS
$previousWorkspace = $env:AGENTERM_WORKSPACE_PATH
$env:AGENTERM_IPC_ADDRESS = "127.0.0.1:$((47000 + ($PID % 1000)))"
$workspaceFile = Join-Path $env:TEMP "agenterm-ux-$PID.json"
$env:AGENTERM_WORKSPACE_PATH = $workspaceFile
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
    $id = $tab.id

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

    Write-Host 'STEP settings discovery and modal'
    $settings = Invoke-AgenTerm @('get-settings') | ConvertFrom-Json
    if ($settings.terminal_font_size -lt 8 -or
        $settings.recommended_cjk_font -ne 'Sarasa Fixed SC') {
        throw 'get-settings did not expose font settings and CJK recommendation'
    }
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
    Write-Host 'PASS: UX state, two-line tabs, settings, composer, focus, safe close, dead close, protocol discovery'
}
finally {
    Remove-Item -LiteralPath $draftFile -ErrorAction SilentlyContinue
    & $Exe kill-server 2>$null | Out-Null
    Remove-Item -LiteralPath $workspaceFile -ErrorAction SilentlyContinue
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
}
